// qcp client event loop
// (c) 2024 Ross Younger

use crate::client::args::ProcessedArgs;
use crate::protocol::control::{ClientMessage, ServerMessage};
use crate::protocol::session::session_capnp::Status;
use crate::protocol::session::{FileHeader, FileTrailer, Response};
use crate::protocol::{RawStreamPair, StreamPair};
use crate::util::{self, lookup_host_by_family, time::StopwatchChain};
use crate::{cert::Credentials, protocol};

use super::ClientArgs;
use anyhow::{Context, Result};
use futures_util::TryFutureExt as _;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{rustls, Connection, EndpointConfig};
use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufReader, BufWriter};
use tokio::process::Child;
use tokio::{self, io::AsyncReadExt, time::timeout, time::Duration};
use tracing::{debug, error, info, span, trace, trace_span, warn, Level};

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

const SHOW_TIME: &str = "file transfer";

/// Main CLI entrypoint
#[tokio::main]
pub async fn client_main(args: ClientArgs, progress: MultiProgress) -> anyhow::Result<bool> {
    // Caution: As we are using ProgressBar, anything to be printed to console should
    // use progress.println() !
    let spinner = progress.add(ProgressBar::new_spinner());
    spinner.set_message("Setting up");
    spinner.enable_steady_tick(Duration::from_millis(150));

    let mut timers = StopwatchChain::default();
    timers.next("setup");
    let unpacked_args = ProcessedArgs::try_from(args)?;
    let args = unpacked_args.original.clone();

    let span = trace_span!("CLIENT");
    let _guard = span.enter();
    let credentials = crate::cert::Credentials::generate()?;

    let host = unpacked_args.remote_host();
    let server_address = lookup_host_by_family(host, args.address_family())?;

    spinner.set_message("Connecting control channel...");
    timers.next("control channel");
    debug!("connecting to remote");
    let mut server = launch_server(&unpacked_args)?;

    wait_for_banner(&mut server, args.timeout).await?;
    let mut server_input = server.stdin.take().unwrap();
    ClientMessage::write(
        &mut server_input,
        &credentials.certificate,
        server_address.into(),
    )
    .await?;

    let mut server_output = server.stdout.take().unwrap();
    trace!("waiting for server message");
    let server_message = ServerMessage::read(&mut server_output).await?;
    debug!(
        "Got server message; cert length {}, port {}, hostname {}, warning {:?}",
        server_message.cert.len(),
        server_message.port,
        server_message.name,
        server_message.warning
    );
    if let Some(w) = server_message.warning {
        warn!("Remote endpoint warning: {w}");
    }

    let server_address_port = match server_address {
        std::net::IpAddr::V4(ip) => SocketAddrV4::new(ip, server_message.port).into(),
        std::net::IpAddr::V6(ip) => SocketAddrV6::new(ip, server_message.port, 0, 0).into(),
    };

    spinner.set_message("Establishing data channel");
    timers.next("quic setup");
    let endpoint = create_endpoint(
        &credentials,
        server_message.cert.into(),
        &server_address_port,
        args.kernel_buffer_size,
    )?;

    debug!("Opening QUIC connection to {server_address_port:?}");
    debug!("Local endpoint address is {:?}", endpoint.local_addr()?);

    let connection_fut = endpoint.connect(server_address_port, &server_message.name)?;
    let timeout_fut = tokio::time::sleep(CONNECTION_TIMEOUT);
    tokio::pin!(connection_fut, timeout_fut);

    let connection = tokio::select! {
        _ = timeout_fut => {
            anyhow::bail!("UDP connection to QUIC endpoint timed out");
        },
        c = &mut connection_fut => {
            match c {
                Ok(conn) => conn,
                Err(e) => {
                    anyhow::bail!("Failed to connect: {e}");
                },
            }
        },
    };
    let connection2 = connection.clone();

    spinner.set_message("Transferring data");
    timers.next(SHOW_TIME);
    let result = manage_request(connection, unpacked_args, progress.clone()).await;
    let total_bytes = match result {
        Ok(b) => b,
        Err(b) => b,
    };

    timers.next("shutdown");
    spinner.set_message("Shutting down");
    debug!("shutting down");
    // close child process stdin, which should trigger its exit
    server_input.shutdown().await?;
    // Forcibly (but gracefully) tear down QUIC. All the requests have completed or errored.
    endpoint.close(1u8.into(), "finished".as_bytes());
    let closedown_fut = endpoint.wait_idle();
    let timeout_fut = tokio::time::sleep(CONNECTION_TIMEOUT);
    tokio::pin!(closedown_fut, timeout_fut);
    tokio::select! {
        _ = timeout_fut => warn!("QUIC shutdown timed out"),
        _ = closedown_fut => (),
    };
    trace!("waiting for child");
    server.wait().await?;
    trace!("finished");
    timers.stop();

    drop(_guard);

    let transport_time = timers.find(SHOW_TIME).and_then(|sw| sw.elapsed());

    if !args.quiet {
        crate::util::stats::output_statistics(
            &args,
            connection2.stats(),
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
    args: ProcessedArgs,
    progress: MultiProgress,
) -> Result<u64, u64> {
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        // This async block returns a Result<u64>
        let sp = connection.open_bi().map_err(|e| anyhow::anyhow!(e)).await?;

        // Called function returns its payload size.
        // This async block reports on errors.
        process_request(sp, args, progress).await
    });

    let mut total_bytes = 0u64;
    let mut success = true;
    loop {
        let result = match tasks.join_next().await {
            Some(res) => res,
            None => break, // all have joined
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

/// Deal with a single request
async fn process_request(
    sp: (quinn::SendStream, quinn::RecvStream),
    args: ProcessedArgs,
    mp: MultiProgress,
) -> anyhow::Result<u64> {
    let progress_bar = mp.add(
        ProgressBar::new(1)
            .with_elapsed(Duration::ZERO)
            .with_finish(ProgressFinish::Abandon),
    );

    let output = console::Term::stderr();

    // The displayed message is the filename part of the source
    let display_filename = {
        let component = PathBuf::from(&args.source.filename);
        component
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };

    progress_bar.set_style(indicatif::ProgressStyle::with_template(
        crate::console::progress_style_for(&output, display_filename.len()),
    )?);
    progress_bar.set_message(display_filename);
    // N.B. The command handler manages the progress bar length field, once it is known.

    let result = if args.source.host.is_some() {
        // This is a Get
        do_get(
            sp,
            &args.source.filename,
            &args.destination.filename,
            &args,
            progress_bar.clone(),
        )
        .await
    } else {
        // This is a Put
        do_put(
            sp,
            &args.source.filename,
            &args.destination.filename,
            &args,
            progress_bar.clone(),
        )
        .await
    };
    if result.is_ok() {
        progress_bar.finish_and_clear();
    }
    result
}

fn launch_server(args: &ProcessedArgs) -> Result<Child> {
    let remote_host = args.remote_host();
    let mut server = tokio::process::Command::new("ssh");
    // TODO extra ssh options
    server.args([
        remote_host,
        "qcpt",
        "-b",
        &args.original.buffer_size.to_string(),
    ]);
    if args.original.remote_debug {
        server.arg("--debug");
    }
    server
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // TODO: pipe this more nicely, output on error?
        .kill_on_drop(true);
    trace!("spawning command: {:?}", server);
    server
        .spawn()
        .context("Could not launch control connection to remote server")
}

async fn wait_for_banner(server: &mut Child, timeout_s: u16) -> Result<()> {
    use protocol::control::BANNER;
    let channel = server.stdout.as_mut().expect("missing server stdout");
    let mut buf = [0u8; BANNER.len()];
    let mut reader = channel.take(buf.len() as u64);
    let n_fut = reader.read(&mut buf);

    let n = timeout(Duration::from_secs(timeout_s.into()), n_fut)
        .await
        .with_context(|| "timed out reading server banner")??;

    let read_banner = std::str::from_utf8(&buf).with_context(|| "bad server banner")?;
    anyhow::ensure!(n != 0, "failed to connect"); // the process closed its stdout
    anyhow::ensure!(BANNER == read_banner, "server banner not as expected");
    Ok(())
}

/// Creates the client endpoint:
/// `credentials` are generated locally.
/// `server_cert` comes from the control channel server message.
/// `destination` is the server's address (port from the control channel server message).
pub fn create_endpoint(
    credentials: &Credentials,
    server_cert: CertificateDer<'_>,
    server_addr: &SocketAddr,
    kernel_buffer_size: usize,
) -> Result<quinn::Endpoint> {
    let span = span!(Level::TRACE, "create_endpoint");
    let _guard = span.enter();
    let mut root_store = RootCertStore::empty();
    root_store.add(server_cert).map_err(|e| {
        error!("{e}");
        e
    })?;
    let root_store = Arc::new(root_store);

    let tls_config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(credentials.cert_chain(), credentials.keypair.clone_key())?,
    );

    let qcc = Arc::new(QuicClientConfig::try_from(tls_config)?);
    let config = quinn::ClientConfig::new(qcc);

    trace!("bind socket");
    let socket = util::socket::bind_unspecified_for(server_addr)?;
    let _ = util::socket::set_udp_buffer_sizes(&socket, kernel_buffer_size)?
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
    cli_args: &ProcessedArgs,
    progress: ProgressBar,
) -> Result<u64> {
    let mut stream: StreamPair = sp.into();
    let span = span!(Level::TRACE, "do_get", filename = filename);
    let _guard = span.enter();

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

    trace!("starting");

    let recv_buf = BufReader::with_capacity(cli_args.original.buffer_size, stream.recv);
    let mut progress_async = progress.wrap_async_read(recv_buf);

    let header = FileHeader::read(&mut progress_async).await?;
    trace!("got {header:?}");

    let mut file = crate::util::io::create_truncate_file(dest, &header).await?;

    progress.set_position(0);
    progress.set_length(header.size);
    let mut limited_recv = progress_async.take(header.size);
    trace!("payload");
    tokio::io::copy_buf(&mut limited_recv, &mut file).await?;

    // stream.recv has been moved but we can get it back for further operations
    progress_async = limited_recv.into_inner();

    trace!("trailer");
    let _trailer = FileTrailer::read(&mut progress_async).await?;
    // Trailer is empty for now, but its existence means the server believes the file was sent correctly

    file.flush().await?;
    stream.send.finish()?;
    trace!("complete");
    Ok(header.size)
}

async fn do_put(
    sp: RawStreamPair,
    src_filename: &str,
    dest_filename: &str,
    cli_args: &ProcessedArgs,
    progress: ProgressBar,
) -> Result<u64> {
    let mut stream: StreamPair = sp.into();

    let span = span!(Level::TRACE, "do_put");
    let _guard = span.enter();

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
    trace!("starting");
    let mut file = BufReader::with_capacity(cli_args.original.file_buffer_size(), file);

    {
        let cmd = crate::protocol::session::Command::new_put(dest_filename);
        let buf = cmd.serialize();
        stream.send.write_all(&buf).await?;
    }
    stream.send.flush().await?;
    let progress_async = progress.wrap_async_write(stream.send);

    // TODO protocol timeout?
    trace!("await response");
    let response = Response::read(&mut stream.recv).await?;
    if response.status != Status::Ok {
        anyhow::bail!(format!("PUT ({src_filename}) failed: {response}"));
    }

    let mut send_buf = BufWriter::with_capacity(cli_args.original.buffer_size, progress_async);

    // The filename in the protocol is the file part only of src_filename
    trace!("send header");
    let protocol_filename = path.file_name().unwrap().to_str().unwrap().to_string(); // can't fail with the preceding checks
    let header = FileHeader::serialize_direct(payload_len, &protocol_filename);
    send_buf.write_all(&header).await?;
    progress.set_position(0);
    progress.set_length(payload_len);

    // A server-side abort might happen part-way through a large transfer.
    trace!("send payload");
    let result = tokio::io::copy_buf(&mut file, &mut send_buf).await;

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
    send_buf.write_all(&trailer).await?;
    send_buf.flush().await?;

    let response = Response::read(&mut stream.recv).await?;
    if response.status != Status::Ok {
        anyhow::bail!(format!(
            "PUT ({src_filename}) failed on completion check: {response}"
        ));
    }

    // TODO: It would be ideal to call finish on the Quinn sendstream within, but it's hard to extract from the ProgressBarIter.
    // send_buf.into_inner().finish();
    trace!("complete");
    Ok(payload_len)
}
