// qcp server event loop
// (c) 2024 Ross Younger

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use crate::cert::Credentials;
use crate::protocol::control::{ClientMessage, ServerMessage};
use crate::protocol::session::session_capnp::Status;
use crate::protocol::session::Command;
use crate::protocol::session::{FileHeader, FileTrailer, Response};
use crate::protocol::{self, StreamPair};
use crate::{transport, util};

use quinn::crypto::rustls::QuicServerConfig;
use quinn::rustls::server::WebPkiClientVerifier;
use quinn::rustls::{self, RootCertStore};
use quinn::EndpointConfig;
use rustls_pki_types::CertificateDer;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, BufReader};
use tokio::task::JoinSet;
use tokio::time::Duration;
use tracing::{debug, error, info, trace, trace_span, warn};

use super::ServerArgs;

const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);

/// Main entrypoint
#[tokio::main]
pub async fn server_main(args: &ServerArgs) -> anyhow::Result<()> {
    let span = trace_span!("SERVER");
    let _guard = span.enter();

    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    // There are tricks you can use to get an unbuffered handle to stdout, but at a typing cost.
    // For now we'll manually flush after each write.

    stdout
        .write_all(protocol::control::BANNER.as_bytes())
        .await?;
    stdout.flush().await?;

    let client_message = ClientMessage::read(&mut stdin).await.unwrap_or_else(|e| {
        // try to be helpful if there's a human reading
        eprintln!("ERROR: This program expects a binary data packet on stdin.\n{e}");
        std::process::exit(1);
    });
    trace!(
        "got client message length {}, using {}",
        client_message.cert.len(),
        client_message.connection_type,
    );

    // TODO: Allow port to be specified
    let credentials = crate::cert::Credentials::generate()?;
    let (endpoint, warning) = create_endpoint(&credentials, client_message, args)?;
    let local_addr = endpoint.local_addr()?;
    debug!("Local address is {local_addr}");
    ServerMessage::write(
        &mut stdout,
        local_addr.port(),
        &credentials.certificate,
        &credentials.hostname,
        warning.as_deref(),
    )
    .await?;
    stdout.flush().await?;

    let mut tasks = JoinSet::new();

    // Control channel main logic:
    // Wait for a successful connection OR timeout OR for stdin to be closed (implicitly handled).
    // We have tight control over what we expect (TLS peer certificate/name) so only need to handle one successful connection,
    // but a timeout is useful to give the user a cue that UDP isn't getting there.
    let endpoint_fut = endpoint.accept();
    let timeout_fut = tokio::time::sleep(PROTOCOL_TIMEOUT);
    tokio::pin!(endpoint_fut, timeout_fut);

    trace!("main select");
    tokio::select! {
        e = &mut endpoint_fut => {
            match e {
                None => {
                    info!("Endpoint was expectedly closed");
                },
                Some(conn) => {
                    let conn_fut = handle_connection(conn, (*args).clone());
                    tasks.spawn(async move {
                        if let Err(e) = conn_fut.await {
                            error!("inward stream failed: {reason}", reason = e.to_string());
                        }
                        trace!("connection completed");
                    });
                },
            };
        },
        _ = &mut timeout_fut => {
            info!("timed out waiting for connection");
        },
    };

    // Graceful closedown. Wait for all connections and streams to finish.
    info!("waiting for completion");
    let _ = tasks.join_all().await;
    endpoint.close(1u8.into(), "finished".as_bytes());
    endpoint.wait_idle().await;
    trace!("finished");
    Ok(())
}

fn create_endpoint(
    credentials: &Credentials,
    client_message: ClientMessage,
    args: &ServerArgs,
) -> anyhow::Result<(quinn::Endpoint, Option<String>)> {
    let client_cert: CertificateDer<'_> = client_message.cert.into();

    let mut root_store = RootCertStore::empty();
    root_store.add(client_cert)?;
    let root_store = Arc::new(root_store);
    let verifier = WebPkiClientVerifier::builder(root_store.clone()).build()?;
    let tls_config = rustls::ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(credentials.cert_chain(), credentials.keypair.clone_key())?;

    // N.B.: in ServerConfig docs, max_early_data_size should be set to u32::MAX

    let qsc = QuicServerConfig::try_from(tls_config)?;
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(qsc));
    config.transport_config(crate::transport::config_factory(
        *args.bandwidth,
        args.rtt,
        args.initial_congestion_window,
    )?);

    // TODO let caller specify port
    let addr = match client_message.connection_type {
        crate::util::AddressFamily::Any => {
            anyhow::bail!("address family Any not supported here (can't happen)")
        }
        crate::util::AddressFamily::IPv4 => {
            SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0)
        }
        crate::util::AddressFamily::IPv6 => {
            SocketAddr::new(std::net::IpAddr::V6(Ipv6Addr::UNSPECIFIED), 0)
        }
    };
    let socket = std::net::UdpSocket::bind(addr)?;
    let warning = util::socket::set_udp_buffer_sizes(
        &socket,
        transport::SEND_BUFFER_SIZE,
        transport::receive_window_for(*args.bandwidth, args.rtt) as usize,
    )?
    .inspect(|s| warn!("{s}"));

    // SOMEDAY: allow user to specify max_udp_payload_size in endpoint config, to support jumbo frames
    let runtime =
        quinn::default_runtime().ok_or_else(|| anyhow::anyhow!("no async runtime found"))?;
    Ok((
        quinn::Endpoint::new(EndpointConfig::default(), Some(config), socket, runtime)?,
        warning,
    ))
}

async fn handle_connection(conn: quinn::Incoming, args: ServerArgs) -> anyhow::Result<()> {
    let span = trace_span!("incoming");
    let _guard = span.enter();

    let connection = conn.await?;
    info!("accepted connection from {}", connection.remote_address());
    let args = Arc::new(args);

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
            let fut = handle_stream(stream, args.clone());
            tokio::spawn(async move {
                if let Err(e) = fut.await {
                    error!("stream failed: {e}",);
                }
            });
        }
    }
    .await?;
    Ok(())
}

async fn handle_stream(mut sp: StreamPair, args: Arc<ServerArgs>) -> anyhow::Result<()> {
    trace!("reading command");
    let cmd = Command::read(&mut sp.recv).await?;
    match cmd {
        Command::Get(get) => handle_get(sp, args, get.filename).await,
        Command::Put(put) => handle_put(sp, args, put.filename).await,
    }
}

async fn handle_get(
    mut stream: StreamPair,
    _args: Arc<ServerArgs>,
    filename: String,
) -> anyhow::Result<()> {
    let span = tracing::span!(tracing::Level::TRACE, "GET", filename = filename);
    let _guard = span.enter();
    debug!("begin");

    let path = PathBuf::from(&filename);
    let (file, meta) = match crate::util::io::open_file(&filename).await {
        Ok(res) => res,
        Err((status, message, _)) => {
            send_response(&mut stream.send, status, message.as_deref()).await?;
            return Ok(());
        }
    };
    if meta.is_dir() {
        send_response(&mut stream.send, Status::ItIsADirectory, None).await?;
        return Ok(());
    }
    let mut file = BufReader::with_capacity(crate::transport::SEND_BUFFER_SIZE * 2, file);

    // We believe we can fulfil this request.
    trace!("responding OK");
    send_response(&mut stream.send, Status::Ok, None).await?;

    let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks

    let header = FileHeader::serialize_direct(meta.len(), protocol_filename);
    stream.send.write_all(&header).await?;

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
    let trailer = FileTrailer::serialize_direct();
    stream.send.write_all(&trailer).await?;
    stream.send.flush().await?;
    trace!("complete");
    Ok(())
}

async fn handle_put(
    mut stream: StreamPair,
    _args: Arc<ServerArgs>,
    destination: String,
) -> anyhow::Result<()> {
    let span = tracing::span!(tracing::Level::TRACE, "PUT");
    let _guard = span.enter();
    debug!("begin, destination={destination}");

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
        if !crate::util::io::dest_is_writeable(&path).await {
            send_response(
                &mut stream.send,
                Status::IncorrectPermissions,
                Some("cannot write to destination"),
            )
            .await?;
            return Ok(());
        }
        // append filename only if it is a directory
        path.is_dir()
    } else {
        // Is it a nonexistent file in a valid directory?
        let mut path_test = path.clone();
        path_test.pop();
        if path_test.as_os_str().is_empty() {
            // We're writing a file to the current working directory, so apply the is_dir writability check
            path_test.push(".");
        }
        if path_test.is_dir() {
            if !crate::util::io::dest_is_writeable(&path_test).await {
                send_response(
                    &mut stream.send,
                    Status::IncorrectPermissions,
                    Some("cannot write to destination"),
                )
                .await?;
                return Ok(());
            }
            // Yes, we can write there; destination path is fully specified.
            false
        } else {
            // No parent directory
            send_response(&mut stream.send, Status::DirectoryDoesNotExist, None).await?;
            return Ok(());
        }
    };

    // So far as we can tell, we believe we can fulfil this request.
    trace!("responding OK");
    send_response(&mut stream.send, Status::Ok, None).await?;

    let header = FileHeader::read(&mut stream.recv).await?;

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
        .set_len(header.size)
        .await
        .inspect_err(|e| error!("Could not set destination file length: {e}"))
        .is_err()
    {
        return Ok(());
    };

    trace!("receiving file payload");
    let mut limited_recv = stream.recv.take(header.size);
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
    let _trailer = FileTrailer::read(&mut stream.recv).await?;
    // TODO: Hash checks

    file.flush().await?;
    send_response(&mut stream.send, Status::Ok, None).await?;
    stream.send.flush().await?;
    trace!("complete");
    Ok(())
}

async fn send_response(
    send: &mut quinn::SendStream,
    status: Status,
    message: Option<&str>,
) -> anyhow::Result<()> {
    let buf = Response::serialize_direct(status, message);
    send.write_all(&buf).await?;
    Ok(())
}
