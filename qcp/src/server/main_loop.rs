// qcp server event loop
// (c) 2024 Ross Younger

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;

use crate::cert::Credentials;
use crate::protocol::control::{control_capnp, ClientMessage};
use crate::protocol::session::session_capnp::Status;
use crate::protocol::session::{FileHeader, FileTrailer, Response};
use crate::protocol::{self, StreamPair};

use capnp::message::ReaderOptions;
use futures_util::io::AsyncReadExt as _;
use futures_util::AsyncWriteExt;
use quinn::crypto::rustls::QuicServerConfig;
use quinn::rustls::server::WebPkiClientVerifier;
use quinn::rustls::{self, RootCertStore};
use rustls_pki_types::CertificateDer;
use tokio::fs;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, Stdin};
use tokio::time::Duration;
use tokio_util::compat::Compat as tokCompat;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, trace, trace_span};

const PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);

/// Main entrypoint
#[tokio::main]
pub async fn server_main() -> anyhow::Result<()> {
    let span = trace_span!("SERVER");
    let _guard = span.enter();

    let mut stdin = tokio::io::stdin().compat();
    let mut stdout = unbuffered_stdout();

    stdout
        .write_all(protocol::control::BANNER.as_bytes())
        .await?;

    let credentials = crate::cert::Credentials::generate()?;
    let client_message = read_client_message(&mut stdin).await.unwrap_or_else(|e| {
        // try to be helpful if there's a human reading
        eprintln!("ERROR: This program expects a binary data packet on stdin.\n{e}");
        std::process::exit(1);
    });
    trace!("got client message length {}", client_message.cert.len());

    // TODO: Allow port to be specified
    let endpoint = create_endpoint(&credentials, client_message.cert.into())?;
    info!("Server endpoint port={}", endpoint.local_addr()?.port());
    {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut server_msg = msg.init_root::<control_capnp::server_message::Builder>();
        server_msg.set_cert(&credentials.certificate);
        server_msg.set_port(endpoint.local_addr()?.port());
        server_msg.set_name(&credentials.hostname);
        trace!("sending server message");
        capnp_futures::serialize::write_message(stdout.compat_write(), msg).await?;
    }

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
                        let conn_fut = handle_connection(conn);
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

#[cfg(unix)]
fn unbuffered_stdout() -> tokio::fs::File {
    use std::os::fd::AsFd;
    let owned = std::io::stdout().as_fd().try_clone_to_owned().unwrap();
    let file = std::fs::File::from(owned);
    tokio::fs::File::from_std(file)
}

async fn read_client_message(stdin: &mut tokCompat<Stdin>) -> anyhow::Result<ClientMessage> {
    debug!("waiting for client message");
    let reader = capnp_futures::serialize::read_message(stdin, ReaderOptions::new()).await?;
    let msg_reader: control_capnp::client_message::Reader = reader.get_root()?;
    let cert = Vec::<u8>::from(msg_reader.get_cert()?);
    Ok(ClientMessage { cert })
}

fn create_endpoint(
    credentials: &Credentials,
    client_cert: CertificateDer<'_>,
) -> anyhow::Result<quinn::Endpoint> {
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
    let addr = SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
    let endpoint = quinn::Endpoint::server(config, addr)?;

    Ok(endpoint)
}

async fn handle_connection(conn: quinn::Incoming) -> anyhow::Result<()> {
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
            let fut = handle_stream(stream);
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

async fn handle_stream(mut sp: StreamPair) -> anyhow::Result<()> {
    use crate::protocol::session::session_capnp::{self, command};
    use crate::protocol::session::{Command, GetArgs, PutArgs};

    let span = tracing::span!(
        tracing::Level::TRACE,
        "stream",
        id = sp.send.get_ref().id().to_string()
    );
    let _guard = span.enter();

    trace!("reading command");
    let reader = capnp_futures::serialize::read_message(&mut sp.recv, ReaderOptions::new()).await?;
    let msg_reader: session_capnp::command::Reader = reader.get_root()?;

    // I can't help but think there should be a better way to do this.
    // If msg_reader is still alive when we call down to an async, compile fails as msg_reader is not Send.
    // To work around this I'm creating objects from msg_reader, which I then proceed to pick apart.
    // Doesn't this defeat the point of capnproto in avoiding allocs?
    // Or do I need to find a clever lifetime spec?
    let cmd: Command = match msg_reader.get_args().which() {
        Ok(command::args::Get(get)) => Command::Get(GetArgs {
            filename: get?.get_filename()?.to_string()?,
        }),
        Ok(command::args::Put(put)) => Command::Put(PutArgs {
            filename: put?.get_filename()?.to_string()?,
        }),
        Err(e) => {
            error!("error reading command: {e}");
            anyhow::bail!("error reading command");
        }
    };
    match cmd {
        Command::Get(args) => handle_get(&mut sp, args.filename).await,
        Command::Put(args) => handle_put(&mut sp, args.filename).await,
    }
}

async fn handle_get(sp: &mut StreamPair, filename: String) -> anyhow::Result<()> {
    debug!("GET {filename}");

    let path = PathBuf::from(&filename);
    let (mut file, meta) = match crate::util::open_file_read(&filename).await {
        Ok(res) => res,
        Err((status, message, _)) => {
            send_response(&mut sp.send, status, message.as_deref()).await?;
            return Ok(());
        }
    };
    if meta.is_dir() {
        send_response(&mut sp.send, Status::ItIsADirectory, None).await?;
        return Ok(());
    }
    // We believe we can fulfil this request.
    send_response(&mut sp.send, Status::Ok, None).await?;

    let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks

    let header = FileHeader::serialize_direct(meta.len(), protocol_filename);
    sp.send.write_all(&header).await?;

    let result = tokio::io::copy(&mut file, sp.send.get_mut()).await;
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
    sp.send.write_all(&trailer).await?;
    sp.send.flush().await?;
    Ok(())
}

async fn dest_is_writeable(dest: &PathBuf) -> bool {
    let meta = fs::metadata(dest).await;
    match meta {
        Ok(m) => !m.permissions().readonly(),
        Err(_) => false,
    }
}

async fn handle_put(sp: &mut StreamPair, destination: String) -> anyhow::Result<()> {
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
                &mut sp.send,
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
                    &mut sp.send,
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
            send_response(&mut sp.send, Status::DirectoryDoesNotExist, None).await?;
            return Ok(());
        }
    };

    // So far as we can tell, we believe we can fulfil this request.
    send_response(&mut sp.send, Status::Ok, None).await?;

    let header = FileHeader::read(&mut sp.recv).await?;
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

    let mut read_n = sp.recv.get_mut().take(header.size);
    if tokio::io::copy(&mut read_n, &mut file)
        .await
        .inspect_err(|e| error!("Failed to write to destination: {e}"))
        .is_err()
    {
        return Ok(());
    }

    let _trailer = FileTrailer::read(&mut sp.recv).await?;
    // TODO: Hash checks

    file.flush().await?;
    send_response(&mut sp.send, Status::Ok, None).await?;
    sp.send.flush().await?;

    Ok(())
}

async fn send_response(
    send: &mut tokCompat<quinn::SendStream>,
    status: Status,
    message: Option<&str>,
) -> anyhow::Result<()> {
    let buf = Response::serialize_direct(status, message);
    send.write_all(&buf).await?;
    Ok(())
}
