// qcp client event loop
// (c) 2024 Ross Younger

use crate::client::args::ProcessedArgs;
use crate::protocol::control::{ClientMessage, ServerMessage};
use crate::protocol::session::session_capnp::Status;
use crate::protocol::session::{FileHeader, FileTrailer, Response};
use crate::protocol::{RawStreamPair, StreamPair};
use crate::util::{lookup_host_by_family, time::StopwatchChain};
use crate::{cert::Credentials, protocol};

use super::ClientArgs;
use anyhow::{Context, Result};
use futures_util::TryFutureExt as _;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{rustls, Connection};
use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
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
        "Got server message; cert length {}, port {}, hostname {}",
        server_message.cert.len(),
        server_message.port,
        server_message.name
    );

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
            result.payload_size,
            transport_time,
        );
    }

    if args.profile {
        info!("Elapsed time by phase:\n{timers}");
    }
    progress.clear()?;
    Ok(result.is_success())
}

#[derive(Default)]
struct RequestResult {
    // If present, we were successful.
    // If not present, we were not successful.
    payload_size: Option<u64>,
}

impl RequestResult {
    fn failure() -> Self {
        Self::default()
    }
    fn success(payload_size: u64) -> Self {
        Self {
            payload_size: Some(payload_size),
        }
    }
    fn is_success(&self) -> bool {
        self.payload_size.is_some()
    }
    /// Merges another result into this one
    fn fold(&mut self, other: Self) {
        self.payload_size = if let Some(a) = self.payload_size {
            other.payload_size.map(|b| a + b)
        } else {
            None
        };
    }
}

async fn manage_request(
    connection: Connection,
    args: ProcessedArgs,
    progress: MultiProgress,
) -> RequestResult {
    // For now there is only one task, but we anticipate moving multiple files in future.
    let mut tasks = tokio::task::JoinSet::new();
    tasks.spawn(async move {
        let sp = match connection.open_bi().map_err(|e| anyhow::anyhow!(e)).await {
            Ok(r) => r,
            Err(e) => {
                error!("{e}");
                return RequestResult::failure();
            }
        };

        // Called function returns its payload size.
        // This function exists only to report any errors and return true/false to show success.
        process_request(sp, args, progress)
            .inspect_err(|e| error!("{e}"))
            .map_ok_or_else(|_| RequestResult::failure(), RequestResult::success)
            .await
    });

    let mut result = RequestResult::success(0);
    loop {
        let rr = match tasks.join_next().await {
            Some(Ok(res)) => res,
            Some(Err(_)) => RequestResult::failure(),
            None => break, // all joined
        };
        result.fold(rr);
    }
    result
}

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

    let addr: SocketAddr = match server_addr {
        SocketAddr::V4(_) => SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into(),
        SocketAddr::V6(_) => SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0).into(),
    };
    trace!("create endpoint");
    let mut endpoint = quinn::Endpoint::client(addr)?;
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
    let progress_async = progress.wrap_async_read(stream.recv);
    let mut recv_buf = BufReader::with_capacity(cli_args.original.buffer_size, progress_async);

    let header = FileHeader::read(&mut recv_buf).await?;
    trace!("got {header:?}");

    let file = crate::util::io::create_truncate_file(dest, &header).await?;
    let mut file_buf = BufWriter::with_capacity(cli_args.original.file_buffer_size(), file);

    progress.set_position(0);
    progress.set_length(header.size);
    let mut limited_recv = recv_buf.take(header.size);
    trace!("payload");
    tokio::io::copy_buf(&mut limited_recv, &mut file_buf).await?;

    // stream.recv has been moved but we can get it back for further operations
    recv_buf = limited_recv.into_inner();

    trace!("trailer");
    let _trailer = FileTrailer::read(&mut recv_buf).await?;
    // Trailer is empty for now, but its existence means the server believes the file was sent correctly

    file_buf.flush().await?;
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
