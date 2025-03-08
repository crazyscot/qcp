//! server-side _(remote)_ event loop
// (c) 2024 Ross Younger

use std::cmp::min;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::{Configuration, Configuration_Optional, Manager};
use crate::protocol::control::{
    BANNER, COMPATIBILITY_LEVEL, ClientGreeting, ClientMessage, ClosedownReport, ClosedownReportV1,
    ConnectionType, ServerFailure, ServerGreeting, ServerMessage, ServerMessageV1,
};
use crate::protocol::session::{Command, FileHeader, FileTrailer, Response, ResponseV1, Status};
use crate::protocol::{common::ProtocolMessage as _, common::StreamPair};
use crate::transport::{ThroughputMode, combine_bandwidth_configurations};
use crate::util::{Credentials, TimeFormat, io, socket};

use anyhow::Context as _;
use quinn::crypto::rustls::QuicServerConfig;
use quinn::rustls::server::WebPkiClientVerifier;
use quinn::rustls::{self, RootCertStore};
use quinn::{ConnectionStats, EndpointConfig};
use rustls_pki_types::CertificateDer;
use serde_bare::Uint;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{Instrument, debug, error, info, trace, trace_span, warn};

fn setup_tracing(debug: bool, time_format: TimeFormat) -> anyhow::Result<()> {
    let level = if debug { "debug" } else { "info" };
    crate::util::setup_tracing(level, None, &None, time_format) // to provoke error: set RUST_LOG=.
}

/// Server event loop
#[allow(clippy::module_name_repetitions)]
#[allow(clippy::too_many_lines)]
pub async fn server_main() -> anyhow::Result<()> {
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();

    // There are tricks you can use to get an unbuffered handle to stdout, but at a typing cost.
    // For now we'll manually flush after each write.

    // PHASE 1: BANNER (checked by client)

    stdout.write_all(BANNER.as_bytes()).await?;
    stdout.flush().await?;

    // PHASE 2: EXCHANGE GREETINGS

    ServerGreeting {
        compatibility: COMPATIBILITY_LEVEL.into(),
        extension: 0,
    }
    .to_writer_async_framed(&mut stdout)
    .await
    .context("sending server greeting")?;
    stdout.flush().await?;

    let remote_greeting = ClientGreeting::from_reader_async_framed(&mut stdin)
        .await
        .context("failed to read client greeting")?;

    let remote_ip = ssh_remote_address();
    let mut manager = Manager::standard(remote_ip.as_deref());
    setup_tracing(
        remote_greeting.debug,
        manager
            .get::<Configuration_Optional>()
            .unwrap_or_default()
            .time_format
            .unwrap_or_default(),
    )?;
    let _span = tracing::error_span!("Server").entered();

    debug!("got client greeting {remote_greeting:?}");
    debug!("client IP is {}", remote_ip.as_ref().map_or("none", |v| v));

    let compat = min(remote_greeting.compatibility.into(), COMPATIBILITY_LEVEL);
    debug!("selected compatibility level {compat}");

    // PHASE 3: EXCHANGE OF MESSAGES

    let client_message = match ClientMessage::from_reader_async_framed(&mut stdin).await {
        Ok(cm) => cm,
        Err(e) => {
            ServerMessage::Failure(ServerFailure::Malformed)
                .to_writer_async_framed(&mut stdout)
                .await?;
            // try to be helpful if there's a human reading
            error!("{e}");
            anyhow::bail!(
                "In server mode, this program expects to receive a binary data packet on stdin"
            );
        }
    };

    trace!("waiting for client message");
    let message1 = match client_message {
        ClientMessage::V1(m) => m,
        ClientMessage::ToFollow => {
            ServerMessage::Failure(ServerFailure::Malformed)
                .to_writer_async_framed(&mut stdout)
                .await?;
            anyhow::bail!("remote or logic error: unpacked unexpected ClientMessage::ToFollow")
        }
    };

    debug!(
        "got client cert length {}, using {:?}",
        message1.cert.len(),
        message1.connection_type,
    );
    //debug!("client msg {message1:?}");
    if message1.show_config {
        info!(
            "Static configuration:\n{}",
            manager.to_display_adapter::<Configuration>()
        );
    }

    let config = match combine_bandwidth_configurations(&mut manager, &message1) {
        Ok(cfg) => cfg,
        Err(e) => {
            ServerMessage::Failure(ServerFailure::NegotiationFailed(format!("{e}")))
                .to_writer_async_framed(&mut stdout)
                .await?;
            return Ok(());
        }
    };

    if message1.show_config {
        info!(
            "Final configuration:\n{}",
            manager.to_display_adapter::<Configuration>()
        );
    }

    let file_buffer_size = usize::try_from(Configuration::send_buffer())?;

    let credentials = Credentials::generate()?;
    let (endpoint, warning) = match create_endpoint(
        &credentials,
        &message1.cert,
        message1.connection_type,
        &config,
    ) {
        Ok(t) => t,
        Err(e) => {
            ServerMessage::Failure(ServerFailure::EndpointFailed(format!("{e}")))
                .to_writer_async_framed(&mut stdout)
                .await?;
            return Ok(());
        }
    };
    let warning = warning.unwrap_or_default();
    let local_addr = endpoint.local_addr()?;
    debug!("Local endpoint address is {local_addr}");
    // FUTURE: When later versions of ServerMessage are created, check client compatibility and send the appropriate version.
    ServerMessage::V1(ServerMessageV1 {
        port: local_addr.port(),
        cert: credentials.certificate.to_vec(),
        name: credentials.hostname,
        bandwidth_to_server: Uint(config.rx()),
        bandwidth_to_client: Uint(config.tx()),
        rtt: config.rtt,
        congestion: config.congestion.into(),
        initial_congestion_window: Uint(config.initial_congestion_window),
        timeout: config.timeout,
        warning,
        extension: 0,
    })
    .to_writer_async_framed(&mut stdout)
    .await?;
    stdout.flush().await?;

    let mut tasks = JoinSet::new();

    // Control channel main logic:
    // Wait for a successful connection OR timeout OR for stdin to be closed (implicitly handled).
    // We have tight control over what we expect (TLS peer certificate/name) so only need to handle one successful connection,
    // but a timeout is useful to give the user a cue that UDP isn't getting there.
    trace!("waiting for QUIC");
    let (stats_tx, mut stats_rx) = oneshot::channel();
    if let Some(conn) = timeout(config.timeout_duration(), endpoint.accept())
        .await
        .context("Timed out waiting for QUIC connection")?
    {
        let _ = tasks.spawn(async move {
            let result = handle_connection(conn, file_buffer_size).await;
            match result {
                Err(e) => error!("inward stream failed: {reason}", reason = e.to_string()),
                Ok(conn_stats) => {
                    let _ = stats_tx.send(conn_stats).inspect_err(|_| {
                        warn!("unable to pass connection stats; possible logic error");
                    });
                }
            }
            trace!("connection completed");
        });
    } else {
        info!("Endpoint was unexpectedly closed");
    }

    // Graceful closedown. Wait for all connections and streams to finish.
    trace!("waiting for completion");
    let _ = tasks.join_all().await;
    endpoint.close(1u8.into(), "finished".as_bytes());
    endpoint.wait_idle().await;
    let stats = stats_rx.try_recv().unwrap_or_default();

    // FUTURE: When later versions of ClosedownReport are created, check client compatibility and send the appropriate version.
    ClosedownReport::V1(ClosedownReportV1::from(&stats))
        .to_writer_async_framed(&mut stdout)
        .await?;
    stdout.flush().await?;
    trace!("finished");
    Ok(())
}

fn create_endpoint(
    our_credentials: &Credentials,
    their_cert: &[u8],
    connection_type: ConnectionType,
    config: &Configuration,
) -> anyhow::Result<(quinn::Endpoint, Option<String>)> {
    let client_cert: CertificateDer<'_> = their_cert.into();

    let mut root_store = RootCertStore::empty();
    root_store.add(client_cert)?;
    let verifier = WebPkiClientVerifier::builder(root_store.into()).build()?;
    let mut tls_config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            our_credentials.cert_chain(),
            our_credentials.keypair.clone_key(),
        )?;
    tls_config.max_early_data_size = u32::MAX;

    let qsc = QuicServerConfig::try_from(tls_config)?;
    let mut server = quinn::ServerConfig::with_crypto(Arc::new(qsc));
    let _ = server.transport_config(crate::transport::create_config(
        config,
        ThroughputMode::Both,
    )?);

    debug!("Using port range {}", config.port);
    let mut socket = socket::bind_range_for_family(connection_type, config.port)?;
    // We don't know whether client will send or receive, so configure for both.
    let wanted_send = Some(usize::try_from(Configuration::send_buffer())?);
    let wanted_recv = Some(usize::try_from(Configuration::recv_buffer())?);
    let warning = socket::set_udp_buffer_sizes(&mut socket, wanted_send, wanted_recv)?
        .inspect(|s| warn!("{s}"));

    // SOMEDAY: allow user to specify max_udp_payload_size in endpoint config, to support jumbo frames
    let runtime =
        quinn::default_runtime().ok_or_else(|| anyhow::anyhow!("no async runtime found"))?;
    Ok((
        quinn::Endpoint::new(EndpointConfig::default(), Some(server), socket, runtime)?,
        warning,
    ))
}

async fn handle_connection(
    conn: quinn::Incoming,
    file_buffer_size: usize,
) -> anyhow::Result<ConnectionStats> {
    let connection = conn.await?;
    debug!(
        "accepted QUIC connection from {}",
        connection.remote_address()
    );

    async {
        loop {
            let stream = connection.accept_bi().await;
            let stream = match stream {
                Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                    // we're closing down
                    debug!("application closing");
                    return Ok::<(), anyhow::Error>(());
                }
                Err(quinn::ConnectionError::ConnectionClosed { .. }) => {
                    debug!("connection closed by remote");
                    return Ok::<(), anyhow::Error>(());
                }
                Err(e) => {
                    error!("connection error: {e}");
                    return Err(e.into());
                }
                Ok(s) => StreamPair::from(s),
            };
            trace!("opened stream");
            let _j = tokio::spawn(async move {
                if let Err(e) = handle_stream(stream, file_buffer_size).await {
                    error!("stream failed: {e}",);
                }
            });
        }
    }
    .await?;
    Ok(connection.stats())
}

async fn handle_stream(mut sp: StreamPair, file_buffer_size: usize) -> anyhow::Result<()> {
    trace!("reading command");
    let cmd = Command::from_reader_async_framed(&mut sp.recv).await?;
    match cmd {
        Command::Get(get) => {
            handle_get(sp, get.filename.clone(), file_buffer_size)
                .instrument(trace_span!("SERVER:GET", filename = get.filename))
                .await
        }
        Command::Put(put) => {
            handle_put(sp, put.filename.clone())
                .instrument(trace_span!("SERVER:PUT", destination = put.filename))
                .await
        }
    }
}

async fn handle_get(
    mut stream: StreamPair,
    filename: String,
    file_buffer_size: usize,
) -> anyhow::Result<()> {
    trace!("begin");

    let path = PathBuf::from(&filename);
    let (file, meta) = match io::open_file(&filename).await {
        Ok(res) => res,
        Err((status, message, _)) => {
            return send_response(&mut stream.send, status, message.as_deref()).await;
        }
    };
    if meta.is_dir() {
        return send_response(&mut stream.send, Status::ItIsADirectory, None).await;
    }
    let mut file = BufReader::with_capacity(file_buffer_size, file);

    // We believe we can fulfil this request.
    trace!("responding OK");
    send_response(&mut stream.send, Status::Ok, None).await?;

    let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks

    FileHeader::new_v1(meta.len(), protocol_filename)
        .to_writer_async_framed(&mut stream.send)
        .await?;

    trace!("sending file payload");
    let result = tokio::io::copy_buf(&mut file, &mut stream.send).await;
    match result {
        Ok(sent) if sent == meta.len() => (),
        Ok(sent) => {
            error!(
                "File sent size {sent} doesn't match its metadata {}",
                meta.len()
            );
            return Ok(());
        }
        Err(e) => {
            error!("Error during io::copy: {e}");
            return Ok(());
        }
    }

    trace!("sending trailer");
    FileTrailer::V1
        .to_writer_async_framed(&mut stream.send)
        .await?;
    stream.send.flush().await?;
    trace!("complete");
    Ok(())
}

async fn handle_put(mut stream: StreamPair, destination: String) -> anyhow::Result<()> {
    trace!("begin");

    // Initial checks. Is the destination valid?
    let mut path = PathBuf::from(destination);
    // This is moderately tricky. It might validly be empty, a directory, a file, it might be a nonexistent file in an extant directory.

    if path.as_os_str().is_empty() {
        // This is the case "qcp some-file host:"
        // Copy to the current working directory
        path.push(".");
    }
    let append_filename = if path.is_dir() || path.is_file() {
        // Destination exists
        if !io::dest_is_writeable(&path).await {
            return send_response(
                &mut stream.send,
                Status::IncorrectPermissions,
                Some("cannot write to destination"),
            )
            .await;
        }
        // append filename only if it is a directory
        path.is_dir()
    } else {
        // Is it a nonexistent file in a valid directory?
        let mut path_test = path.clone();
        let _ = path_test.pop();
        if path_test.as_os_str().is_empty() {
            // We're writing a file to the current working directory, so apply the is_dir writability check
            path_test.push(".");
        }
        if path_test.is_dir() {
            if !io::dest_is_writeable(&path_test).await {
                return send_response(
                    &mut stream.send,
                    Status::IncorrectPermissions,
                    Some("cannot write to destination"),
                )
                .await;
            }
            // Yes, we can write there; destination path is fully specified.
            false
        } else {
            // No parent directory
            return send_response(&mut stream.send, Status::DirectoryDoesNotExist, None).await;
        }
    };

    // So far as we can tell, we believe we can fulfil this request.
    trace!("responding OK");
    let ((), header) = tokio::try_join!(
        send_response(&mut stream.send, Status::Ok, None),
        FileHeader::from_reader_async_framed(&mut stream.recv)
    )?;
    let FileHeader::V1(header) = header;

    debug!("PUT {} -> destination", &header.filename);
    if append_filename {
        path.push(header.filename);
    }
    let mut file = match tokio::fs::File::create(path).await {
        Ok(f) => f,
        Err(e) => {
            error!("Could not write to destination: {e}");
            return Ok(());
        }
    };
    if file
        .set_len(header.size.0)
        .await
        .inspect_err(|e| error!("Could not set destination file length: {e}"))
        .is_err()
    {
        return Ok(());
    };

    trace!("receiving file payload");
    let mut limited_recv = stream.recv.take(header.size.0);
    if tokio::io::copy(&mut limited_recv, &mut file)
        .await
        .inspect_err(|e| error!("Failed to write to destination: {e}"))
        .is_err()
    {
        return Ok(());
    }
    // recv_buf has been moved but we can get it back for further operations
    stream.recv = limited_recv.into_inner();

    trace!("receiving trailer");
    let _trailer = FileTrailer::from_reader_async_framed(&mut stream.recv).await?;

    let f = file.flush();
    send_response(&mut stream.send, Status::Ok, None).await?;
    tokio::try_join!(f, stream.send.flush())?;
    trace!("complete");
    Ok(())
}

async fn send_response(
    send: &mut quinn::SendStream,
    status: Status,
    message: Option<&str>,
) -> anyhow::Result<()> {
    Response::V1(ResponseV1::new(
        status,
        message.map(std::string::ToString::to_string),
    ))
    .to_writer_async_framed(send)
    .await
}

/// Attempts to read the ssh client's IP address.
///
/// This relies on standard OpenSSH behaviour, which is to set environment variables.
/// Returns `None` if the remote address could not be determined.
fn ssh_remote_address() -> Option<String> {
    let env = std::env::var("SSH_CONNECTION");
    if let Ok(s) = env {
        // SSH_CONNECTION: client IP, client port, server IP, server port
        let it = s.split(' ').next();
        if let Some(client) = it {
            return Some(client.to_string());
        }
    }
    let env = std::env::var("SSH_CLIENT");
    if let Ok(s) = env {
        // SSH_CLIENT: client IP, client port, server port
        let it = s.split(' ').next();
        if let Some(client) = it {
            return Some(client.to_string());
        }
    }
    warn!(
        "no SSH_CONNECTION or SSH_CLIENT in environment; not attempting remote-specific configuration"
    );
    None
}
