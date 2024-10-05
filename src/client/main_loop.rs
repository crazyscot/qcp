// qcp client event loop
// (c) 2024 Ross Younger

use crate::cert::Credentials;
use crate::cli::{CliArgs, UnpackedArgs};
use crate::client::control::ControlChannel;
use crate::protocol::session::session_capnp::Status;
use crate::protocol::session::{FileHeader, FileTrailer, Response};
use crate::protocol::{RawStreamPair, StreamPair};
use crate::transport;
use crate::util::time::Stopwatch;
use crate::util::{self, lookup_host_by_family, time::StopwatchChain};

use anyhow::{Context, Result};
use futures_util::TryFutureExt as _;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{rustls, Connection, EndpointConfig};
use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufReader};
use tokio::time::Instant;
use tokio::{self, io::AsyncReadExt, time::timeout, time::Duration};
use tracing::{debug, error, info, span, trace, trace_span, warn, Instrument as _, Level};

const SHOW_TIME: &str = "file transfer";

/// Main CLI entrypoint
// Caution: As we are using ProgressBar, anything to be printed to console should use progress.println() !
pub(crate) async fn client_main(args: &CliArgs, progress: &MultiProgress) -> anyhow::Result<bool> {
    let guard = trace_span!("CLIENT").entered();
    let processed_args = UnpackedArgs::try_from(args)?;
    let spinner = progress.add(ProgressBar::new_spinner());
    spinner.set_message("Setting up");
    spinner.enable_steady_tick(Duration::from_millis(150));
    let mut timers = StopwatchChain::new_running("setup");

    // Prep --------------------------
    let credentials = crate::cert::Credentials::generate()?;
    let host = args.remote_host()?;
    let server_address = lookup_host_by_family(host, args.address_family())?;

    // Control channel ---------------
    spinner.set_message("Connecting control channel...");
    timers.next("control channel");
    let (mut control, server_message) =
        ControlChannel::transact(&args.try_into()?, &credentials, server_address).await?;
    debug!("Got server message {server_message:?}");
    if let Some(w) = server_message.warning {
        warn!("Remote endpoint warning: {w}");
    }

    // Data channel ------------------
    let server_address_port = match server_address {
        std::net::IpAddr::V4(ip) => SocketAddrV4::new(ip, server_message.port).into(),
        std::net::IpAddr::V6(ip) => SocketAddrV6::new(ip, server_message.port, 0, 0).into(),
    };

    spinner.set_message("Establishing data channel");
    timers.next("data channel setup");
    let endpoint = create_endpoint(
        &credentials,
        server_message.cert.into(),
        &server_address_port,
        args,
    )?;
    debug!(
        "Remote endpoint network config: {}",
        server_message.bandwidth_info
    );
    debug!("Opening QUIC connection to {server_address_port:?}");
    debug!("Local endpoint address is {:?}", endpoint.local_addr()?);
    let connection = timeout(
        args.timeout,
        endpoint.connect(server_address_port, &server_message.name)?,
    )
    .await
    .with_context(|| "UDP connection to QUIC endpoint timed out")??;
    let connection2 = connection.clone();

    // Show time! ---------------------
    spinner.set_message("Transferring data");
    timers.next(SHOW_TIME);
    let result = manage_request(connection, processed_args, progress.clone()).await;
    let total_bytes = match result {
        Err(b) | Ok(b) => b,
    };

    // Closedown ----------------------
    timers.next("shutdown");
    spinner.set_message("Shutting down");
    // Forcibly (but gracefully) tear down QUIC. All the requests have completed or errored.
    endpoint.close(1u8.into(), "finished".as_bytes());
    let control_fut = control.close();
    let _ = timeout(args.timeout, endpoint.wait_idle())
        .await
        .inspect_err(|_| warn!("QUIC shutdown timed out")); // otherwise ignore errors
    trace!("QUIC closed; waiting for control channel");
    let _ = timeout(args.timeout, control_fut)
        .await
        .inspect_err(|_| warn!("control channel timed out"));
    // Ignore errors. If the control channel closedown times out, we expect its drop handler will do the Right Thing.

    trace!("finished");
    timers.stop();
    drop(guard);

    // Post-transfer chatter -----------
    if !args.quiet {
        let transport_time = timers.find(SHOW_TIME).and_then(Stopwatch::elapsed);
        crate::util::stats::output_statistics(
            args,
            &connection2.stats(),
            total_bytes,
            transport_time,
        );
    }

    if args.profile {
        info!("Elapsed time by phase:\n{timers}");
    }
    progress.clear()?;
    Ok(result.is_ok())
}

/// Do whatever it is we were asked to.
/// On success: returns the number of bytes transferred.
/// On error: returns the number of bytes that were transferred, as far as we know.
async fn manage_request(
    connection: Connection,
    processed: UnpackedArgs,
    mp: MultiProgress,
) -> Result<u64, u64> {
    let mut tasks = tokio::task::JoinSet::new();
    let _jh = tasks.spawn(async move {
        // This async block returns a Result<u64>
        let sp = connection.open_bi().map_err(|e| anyhow::anyhow!(e)).await?;
        // Called function returns its payload size.
        // This async block reports on errors.
        if processed.source.host.is_some() {
            // This is a Get
            do_get(
                sp,
                &processed.source.filename,
                &processed.destination.filename,
                &processed,
                mp.clone(),
            )
            .instrument(trace_span!("GET", filename = processed.source.filename))
            .await
        } else {
            // This is a Put
            do_put(
                sp,
                &processed.source.filename,
                &processed.destination.filename,
                &processed,
                mp.clone(),
            )
            .instrument(trace_span!("PUT", filename = processed.source.filename))
            .await
        }
    });

    let mut total_bytes = 0u64;
    let mut success = true;
    loop {
        let Some(result) = tasks.join_next().await else {
            break;
        };
        // The first layer of possible errors are Join errors
        let result = match result {
            Ok(r) => r,
            Err(err) => {
                // This is either a panic, or a cancellation.
                if let Ok(reason) = err.try_into_panic() {
                    // Resume the panic on the main task
                    std::panic::resume_unwind(reason);
                } else {
                    // task cancellation (not currently in use, but might be later; this is conceptually benign)
                    warn!("unexpected task join failure (shouldn't happen)");
                    Ok(0)
                }
            }
        };

        // The second layer of possible errors are failures in the protocol. Continue with other jobs as far as possible.
        match result {
            Ok(size) => total_bytes += size,
            Err(e) => {
                error!("{e}");
                success = false;
            }
        }
    }
    if success {
        Ok(total_bytes)
    } else {
        Err(total_bytes)
    }
}

fn progress_bar_for(mp: &MultiProgress, args: &UnpackedArgs, steps: u64) -> Result<ProgressBar> {
    let display_filename = {
        let component = PathBuf::from(&args.source.filename);
        component
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };
    Ok(mp.add(
        ProgressBar::new(steps)
            .with_style(indicatif::ProgressStyle::with_template(
                crate::console::progress_style_for(
                    &console::Term::stderr(),
                    display_filename.len(),
                ),
            )?)
            .with_message(display_filename)
            .with_finish(ProgressFinish::Abandon),
    ))
}

/// Creates the client endpoint:
/// `credentials` are generated locally.
/// `server_cert` comes from the control channel server message.
/// `destination` is the server's address (port from the control channel server message).
pub(crate) fn create_endpoint(
    credentials: &Credentials,
    server_cert: CertificateDer<'_>,
    server_addr: &SocketAddr,
    args: &CliArgs,
) -> Result<quinn::Endpoint> {
    let _ = span!(Level::TRACE, "create_endpoint").entered();
    let mut root_store = RootCertStore::empty();
    root_store.add(server_cert).map_err(|e| {
        error!("{e}");
        e
    })?;
    let bandwidth_bytes: u64 = args.bandwidth.size();

    let tls_config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(credentials.cert_chain(), credentials.keypair.clone_key())?,
    );

    let mut config = quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls_config)?));
    let _ = config.transport_config(crate::transport::create_config(
        bandwidth_bytes,
        args.rtt,
        args.initial_congestion_window,
    )?);

    trace!("bind socket");
    let socket = util::socket::bind_unspecified_for(server_addr)?;
    #[allow(clippy::cast_possible_truncation)]
    let _ = util::socket::set_udp_buffer_sizes(
        &socket,
        transport::SEND_BUFFER_SIZE,
        transport::receive_window_for(bandwidth_bytes, args.rtt) as usize,
        bandwidth_bytes,
        args.rtt,
    )?
    .inspect(|msg| warn!("{msg}"));

    trace!("create endpoint");
    // SOMEDAY: allow user to specify max_udp_payload_size in endpoint config, to support jumbo frames
    let runtime =
        quinn::default_runtime().ok_or_else(|| anyhow::anyhow!("no async runtime found"))?;
    let mut endpoint = quinn::Endpoint::new(EndpointConfig::default(), None, socket, runtime)?;
    endpoint.set_default_client_config(config);

    Ok(endpoint)
}

async fn do_get(
    sp: RawStreamPair,
    filename: &str,
    dest: &str,
    cli_args: &UnpackedArgs,
    multi_progress: MultiProgress,
) -> Result<u64> {
    let mut stream: StreamPair = sp.into();
    let real_start = Instant::now();
    trace!("send command");
    {
        let cmd = crate::protocol::session::Command::new_get(filename);
        let buf = cmd.serialize();
        stream.send.write_all(&buf).await?;
    }
    stream.send.flush().await?;

    // TODO protocol timeout?
    trace!("await response");
    let response = Response::read(&mut stream.recv).await?;
    if response.status != Status::Ok {
        anyhow::bail!(format!("GET ({filename}) failed: {response}"));
    }

    let header = FileHeader::read(&mut stream.recv).await?;
    trace!("{header:?}");

    let mut file = crate::util::io::create_truncate_file(dest, &header).await?;

    // Now we know how much we're receiving, update the chrome.
    // File Trailers are currently 16 bytes on the wire.

    // Unfortunately, the file data is already well in flight at this point, leading to a flood of packets
    // that causes the estimated rate to spike unhelpfully at the beginning of the transfer.
    // Therefore we incorporate time in flight so far to get the estimate closer to reality.
    let progress_bar = progress_bar_for(&multi_progress, cli_args, header.size + 16)?
        .with_elapsed(Instant::now().duration_since(real_start));

    let inbound = progress_bar.wrap_async_read(stream.recv);

    let mut inbound = inbound.take(header.size);
    trace!("payload");
    let _ = tokio::io::copy(&mut inbound, &mut file).await?;
    // Retrieve the stream from within the Take wrapper for further operations
    let mut inbound = inbound.into_inner();

    trace!("trailer");
    let _trailer = FileTrailer::read(&mut inbound).await?;
    // Trailer is empty for now, but its existence means the server believes the file was sent correctly

    // Note that the Quinn send stream automatically calls finish on drop.
    file.flush().await?;
    trace!("complete");
    progress_bar.finish_and_clear();
    Ok(header.size)
}

async fn do_put(
    sp: RawStreamPair,
    src_filename: &str,
    dest_filename: &str,
    cli_args: &UnpackedArgs,
    multi_progress: MultiProgress,
) -> Result<u64> {
    let mut stream: StreamPair = sp.into();

    let path = PathBuf::from(src_filename);
    let (file, meta) = match crate::util::io::open_file(src_filename).await {
        Ok(res) => res,
        Err((_, _, error)) => {
            return Err(error.into());
        }
    };
    if meta.is_dir() {
        anyhow::bail!("PUT: Source is a directory");
    }

    let payload_len = meta.len();

    // Now we can compute how much we're going to send, update the chrome.
    // Marshalled commands are currently 48 bytes + filename length
    // File headers are currently 36 + filename length; Trailers are 16 bytes.
    let steps = payload_len + 48 + 36 + 16 + 2 * dest_filename.len() as u64;
    let progress_bar = progress_bar_for(&multi_progress, cli_args, steps)?;
    let mut outbound = progress_bar.wrap_async_write(stream.send);

    trace!("sending command");
    let mut file = BufReader::with_capacity(crate::transport::SEND_BUFFER_SIZE * 2, file);

    {
        let cmd = crate::protocol::session::Command::new_put(dest_filename);
        let buf = cmd.serialize();
        outbound.write_all(&buf).await?;
    }
    outbound.flush().await?;

    // TODO protocol timeout?
    trace!("await response");
    let response = Response::read(&mut stream.recv).await?;
    if response.status != Status::Ok {
        anyhow::bail!(format!("PUT ({src_filename}) failed: {response}"));
    }

    // The filename in the protocol is the file part only of src_filename
    trace!("send header");
    let protocol_filename = path.file_name().unwrap().to_str().unwrap().to_string(); // can't fail with the preceding checks
    let header = FileHeader::serialize_direct(payload_len, &protocol_filename);
    outbound.write_all(&header).await?;

    // A server-side abort might happen part-way through a large transfer.
    trace!("send payload");
    let result = tokio::io::copy_buf(&mut file, &mut outbound).await;

    match result {
        Ok(sent) if sent == meta.len() => (),
        Ok(sent) => {
            anyhow::bail!(
                "File sent size {sent} doesn't match its metadata {}",
                meta.len()
            );
        }
        Err(e) => {
            if e.kind() == tokio::io::ErrorKind::ConnectionReset {
                // Maybe the connection was cut, maybe the server sent something to help us inform the user.
                let response = match Response::read(&mut stream.recv).await {
                    Err(_) => anyhow::bail!("connection closed unexpectedly"),
                    Ok(r) => r,
                };
                if let Some(msg) = response.message {
                    anyhow::bail!("remote closed connection: {:?}: {}", response.status, msg);
                }
                anyhow::bail!("remote closed connection: {:?}", response.status);
            }
            anyhow::bail!(
                "Unknown I/O error during PUT: {e}/{:?}/{:?}",
                e.kind(),
                e.raw_os_error()
            );
        }
    }

    trace!("send trailer");
    let trailer = FileTrailer::serialize_direct();
    outbound.write_all(&trailer).await?;
    outbound.flush().await?;

    let response = Response::read(&mut stream.recv).await?;
    if response.status != Status::Ok {
        anyhow::bail!(format!(
            "PUT ({src_filename}) failed on completion check: {response}"
        ));
    }

    // Note that the Quinn sendstream calls finish() on drop.
    trace!("complete");
    progress_bar.finish_and_clear();
    Ok(payload_len)
}
