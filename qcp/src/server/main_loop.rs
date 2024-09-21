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

use quinn::crypto::rustls::QuicServerConfig;
use quinn::rustls::server::WebPkiClientVerifier;
use quinn::rustls::{self, RootCertStore};
use rustls_pki_types::CertificateDer;
use tokio::fs;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, BufReader, BufWriter};
use tokio::time::Duration;
use tracing::{debug, error, info, trace, trace_span};

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
    let endpoint = create_endpoint(&credentials, client_message)?;
    let local_addr = endpoint.local_addr()?;
    info!("{local_addr}");
    ServerMessage::write(
        &mut stdout,
        local_addr.port(),
        &credentials.certificate,
        &credentials.hostname,
    )
    .await?;
    stdout.flush().await?;

    loop {
        // Control channel main loop.
        // Wait for new connections OR for stdin to be closed.

        let mut buf = [0u8; 1];
        let endpoint_fut = endpoint.accept();
        let stdin_fut = stdin.read(&mut buf);
        let timeout_fut = tokio::time::sleep(PROTOCOL_TIMEOUT);
        tokio::pin!(endpoint_fut, stdin_fut, timeout_fut);

        tokio::select! {
            s = &mut stdin_fut => {
                match s {
                    Ok(0) => {
                        debug!("stdin was closed");
                        break;
                    }
                    Ok(_) => (), // ignore any data
                    Err(e) => { // can't happen but treat as if closed
                        debug!("error reading stdin: {e}");
                        break;
                    }
                };
            },
            e = &mut endpoint_fut => {
                match e {
                    None => {
                        debug!("Endpoint future returned None");
                        break;
                    },
                    Some(conn) => {
                        let conn_fut = handle_connection(conn, *args);
                        tokio::spawn(async move {
                            if let Err(e) = conn_fut.await {
                                error!("inward stream failed: {reason}", reason = e.to_string());
                            }
                        });
                    },
                };
            },
            _ = &mut timeout_fut => {
                break;
            },
        };
    }

    // Graceful closedown. Wait for all connections and streams to finish.
    info!("waiting for completion");
    endpoint.wait_idle().await;
    trace!("finished");
    Ok(())
}

fn create_endpoint(
    credentials: &Credentials,
    client_message: ClientMessage,
) -> anyhow::Result<quinn::Endpoint> {
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
    let config = quinn::ServerConfig::with_crypto(Arc::new(qsc));

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
    Ok(quinn::Endpoint::server(config, addr)?)
}

async fn handle_connection(conn: quinn::Incoming, args: ServerArgs) -> anyhow::Result<()> {
    let span = trace_span!("incoming");
    let _guard = span.enter();

    let connection = conn.await?;
    info!("accepted connection from {}", connection.remote_address());

    async {
        loop {
            let stream = connection.accept_bi().await;
            let stream = match stream {
                Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                    // we're closing down
                    return Ok::<(), anyhow::Error>(());
                }
                Err(quinn::ConnectionError::ConnectionClosed { .. }) => {
                    info!("remote closed connection");
                    return Ok::<(), anyhow::Error>(());
                }
                Err(e) => {
                    error!("connection error: {e}");
                    return Err(e.into());
                }
                Ok(s) => StreamPair::from(s),
            };
            trace!("opened stream");
            let fut = handle_stream(stream, args);
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

async fn handle_stream(mut sp: StreamPair, args: ServerArgs) -> anyhow::Result<()> {
    let span = tracing::span!(
        tracing::Level::TRACE,
        "stream",
        id = sp.send.id().to_string()
    );
    let _guard = span.enter();

    trace!("reading command");
    let cmd = Command::read(&mut sp.recv).await?;
    match cmd {
        Command::Get(get) => handle_get(sp, &args, get.filename).await,
        Command::Put(put) => handle_put(sp, &args, put.filename).await,
    }
}

async fn handle_get(
    mut stream: StreamPair,
    args: &ServerArgs,
    filename: String,
) -> anyhow::Result<()> {
    debug!("GET {filename}");

    let path = PathBuf::from(&filename);
    let (file, meta) = match crate::util::open_file_read(&filename).await {
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
    let mut file = BufReader::with_capacity(args.file_buffer_size(), file);

    // We believe we can fulfil this request.
    send_response(&mut stream.send, Status::Ok, None).await?;

    let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks

    let mut write_buf = BufWriter::with_capacity(args.buffer_size, stream.send);

    let header = FileHeader::serialize_direct(meta.len(), protocol_filename);
    write_buf.write_all(&header).await?;

    let result = tokio::io::copy_buf(&mut file, &mut write_buf).await;
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

    let trailer = FileTrailer::serialize_direct();
    write_buf.write_all(&trailer).await?;
    write_buf.flush().await?;
    Ok(())
}

async fn dest_is_writeable(dest: &PathBuf) -> bool {
    let meta = fs::metadata(dest).await;
    match meta {
        Ok(m) => !m.permissions().readonly(),
        Err(_) => false,
    }
}

async fn handle_put(
    mut stream: StreamPair,
    args: &ServerArgs,
    destination: String,
) -> anyhow::Result<()> {
    let span = trace_span!("handle_put");
    let _guard = span.enter();
    debug!("destination {destination}"); // this might be a file or a directory

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
        if !dest_is_writeable(&path).await {
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
            if !dest_is_writeable(&path_test).await {
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
    send_response(&mut stream.send, Status::Ok, None).await?;

    let mut recv_buf = BufReader::with_capacity(args.buffer_size, stream.recv);
    let header = FileHeader::read(&mut recv_buf).await?;
    trace!("PUT: HEADER {header:?}");

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

    let mut limited_recv = recv_buf.take(header.size);
    if tokio::io::copy_buf(&mut limited_recv, &mut file)
        .await
        .inspect_err(|e| error!("Failed to write to destination: {e}"))
        .is_err()
    {
        return Ok(());
    }
    // recv_buf has been moved but we can get it back for further operations
    recv_buf = limited_recv.into_inner();

    let _trailer = FileTrailer::read(&mut recv_buf).await?;
    // TODO: Hash checks

    file.flush().await?;
    send_response(&mut stream.send, Status::Ok, None).await?;
    stream.send.flush().await?;

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
