// qcp client event loop
// (c) 2024 Ross Younger

use crate::client::args::ProcessedArgs;
use crate::protocol::control::{control_capnp, ServerMessage};
use crate::protocol::session::session_capnp::Status;
use crate::protocol::session::{session_capnp, FileHeader, FileTrailer, Response};
use crate::protocol::{RawStreamPair, StreamPair};
use crate::util::{lookup_host_by_family, AddressFamily};
use crate::{cert::Credentials, protocol};

use super::ClientArgs;
use anyhow::{Context, Result};
use capnp::message::ReaderOptions;
use futures_util::{FutureExt, TryFutureExt};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{rustls, Connection};
use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, Command};
use tokio::{self, io::AsyncReadExt, time::timeout, time::Duration};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};
use tracing::{debug, error, info, span, trace, trace_span, warn, Level};

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// Main CLI entrypoint
#[tokio::main]
pub async fn client_main(args: &ClientArgs) -> anyhow::Result<bool> {
    let unpacked_args = ProcessedArgs::try_from(args)?;
    //println!("{unpacked_args:?}"); // TEMP

    let span = trace_span!("CLIENT");
    let _guard = span.enter();
    let credentials = crate::cert::Credentials::generate()?;

    let host = unpacked_args.remote_host();
    let server_address = lookup_host_by_family(host, AddressFamily::Any)?;

    info!("connecting to remote");
    let mut server = launch_server(&unpacked_args)?;

    wait_for_banner(&mut server, args.timeout).await?;

    let mut server_input = server.stdin.take().unwrap().compat_write();
    {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut client_msg = msg.init_root::<control_capnp::client_message::Builder>();
        client_msg.set_cert(&credentials.certificate);
        trace!("sending client message");
        capnp_futures::serialize::write_message(&mut server_input, msg).await?;
    }

    let server_output = server.stdout.take().unwrap();
    let server_message = {
        trace!("waiting for server message");
        let reader =
            capnp_futures::serialize::read_message(server_output.compat(), ReaderOptions::new())
                .await?;
        let msg_reader: control_capnp::server_message::Reader = reader.get_root()?;
        let cert = Vec::<u8>::from(msg_reader.get_cert()?);
        let port = msg_reader.get_port();
        let name = msg_reader.get_name()?.to_string()?;
        ServerMessage { port, cert, name }
    };
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

    let endpoint = create_endpoint(
        &credentials,
        server_message.cert.into(),
        &server_address_port,
    )?;

    trace!("Connecting to {server_address_port:?}");
    trace!("Local connection address is {:?}", endpoint.local_addr()?);

    let connection_fut = endpoint.connect(server_address_port, &server_message.name)?;
    let timeout_fut = tokio::time::sleep(CONNECTION_TIMEOUT);
    tokio::pin!(connection_fut, timeout_fut);

    let mut connection = tokio::select! {
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

    let success = manage_request(&mut connection, &unpacked_args).await;

    info!("shutting down");
    // close child process stdin, which should trigger its exit
    server_input.into_inner().shutdown().await?;
    // Forcibly (but gracefully) tear down QUIC. All the requests have completed or errored.
    connection.close(0u8.into(), "".as_bytes());
    let closedown_fut = endpoint.wait_idle().then(|_| server.wait());
    let timeout_fut = tokio::time::sleep(CONNECTION_TIMEOUT);
    tokio::pin!(closedown_fut, timeout_fut);
    tokio::select! {
        _ = timeout_fut => warn!("shutdown timed out"),
        _ = closedown_fut => (),
    };
    Ok(success)
}

async fn manage_request(connection: &mut Connection, args: &ProcessedArgs<'_>) -> bool {
    // TODO: This may spawn, if there are multiple files to transfer.

    // Called function is responsible for tracing errors.
    // We return a simple true/false to show success.
    connection
        .open_bi()
        .map_err(|e| anyhow::anyhow!(e))
        .and_then(|sp| process_request(sp, args))
        .inspect_err(|e| error!("{e}"))
        .map_ok_or_else(|_| false, |_| true)
        .await
}

async fn process_request(
    sp: (quinn::SendStream, quinn::RecvStream),
    args: &ProcessedArgs<'_>,
) -> anyhow::Result<()> {
    if args.source.host.is_some() {
        // This is a Get
        do_get(sp, &args.source.filename, &args.destination.filename, args).await
    } else {
        do_put(sp, &args.source.filename, &args.destination.filename, args).await
    }
}

fn launch_server(args: &ProcessedArgs) -> Result<Child> {
    let remote_host = args.remote_host();
    let mut server = Command::new("ssh");
    // TODO extra ssh options
    server.args([
        remote_host,
        "qcpt",
        "-b",
        &args.original.buffer_size.to_string(),
    ]);
    if args.original.server_debug {
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
    trace!("Set up root store");
    let mut root_store = RootCertStore::empty();
    root_store.add(server_cert).map_err(|e| {
        error!("{e}");
        e
    })?;
    let root_store = Arc::new(root_store);

    trace!("create tls_config");
    let tls_config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(credentials.cert_chain(), credentials.keypair.clone_key())?,
    );

    trace!("create client config");
    let qcc = Arc::new(QuicClientConfig::try_from(tls_config)?);
    let config = quinn::ClientConfig::new(qcc);

    trace!("create endpoint");
    let addr: SocketAddr = match server_addr {
        SocketAddr::V4(_) => SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into(),
        SocketAddr::V6(_) => SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0).into(),
    };
    let mut endpoint = quinn::Endpoint::client(addr)?;
    endpoint.set_default_client_config(config);

    Ok(endpoint)
}

async fn do_get(
    sp: RawStreamPair,
    filename: &str,
    dest: &str,
    cli_args: &ProcessedArgs<'_>,
) -> Result<()> {
    let mut stream: StreamPair = sp.into();

    let span = span!(Level::TRACE, "do_get");
    let _guard = span.enter();

    let mut msg = ::capnp::message::Builder::new_default();
    {
        let command_msg = msg.init_root::<session_capnp::command::Builder>();
        let mut args = command_msg.init_args().init_get();
        args.set_filename(filename);
    }
    trace!("send GET");
    let buf = capnp::serialize::write_message_to_words(&msg);
    stream.send.write_all(&buf).await?;

    // TODO protocol timeout?
    trace!("await response");
    let response = Response::read(&mut stream.recv).await?;
    trace!("GET {response:?}");

    if response.status != Status::Ok {
        anyhow::bail!(format!("GET ({filename}) failed: {response}"));
    }

    let mut recv_buf = BufReader::with_capacity(cli_args.original.buffer_size, stream.recv);

    let header = FileHeader::read(&mut recv_buf).await?;
    trace!("GET: HEADER {header:?}");

    let file = crate::util::open_file_write(dest, &header).await?;
    let mut file_buf = BufWriter::with_capacity(cli_args.original.file_buffer_size(), file);

    let mut limited_recv = recv_buf.take(header.size);
    tokio::io::copy_buf(&mut limited_recv, &mut file_buf).await?;

    // stream.recv has been moved but we can get it back for further operations
    recv_buf = limited_recv.into_inner();

    let _trailer = FileTrailer::read(&mut recv_buf).await?;
    // Trailer is empty for now, but its existence means the server believes the file was sent correctly

    file_buf.flush().await?;
    Ok(())
}

async fn do_put(
    sp: RawStreamPair,
    src_filename: &str,
    dest_filename: &str,
    cli_args: &ProcessedArgs<'_>,
) -> Result<()> {
    let mut stream: StreamPair = sp.into();

    let span = span!(Level::TRACE, "do_put");
    let _guard = span.enter();

    let path = PathBuf::from(src_filename);
    let (file, meta) = match crate::util::open_file_read(src_filename).await {
        Ok(res) => res,
        Err((_, _, error)) => {
            return Err(error.into());
        }
    };
    if meta.is_dir() {
        anyhow::bail!("PUT: Source is a directory");
    }
    let mut file = BufReader::with_capacity(cli_args.original.file_buffer_size(), file);

    let mut msg = ::capnp::message::Builder::new_default();
    {
        let command_msg = msg.init_root::<session_capnp::command::Builder>();
        let mut args = command_msg.init_args().init_put();
        args.set_filename(dest_filename);
    }
    trace!("send PUT");
    let buf = capnp::serialize::write_message_to_words(&msg);
    stream.send.write_all(&buf).await?;

    // TODO protocol timeout?
    trace!("await response");
    let response = Response::read(&mut stream.recv).await?;
    trace!("PUT -> {response:?}");

    if response.status != Status::Ok {
        anyhow::bail!(format!("PUT ({src_filename}) failed: {response}"));
    }

    let mut send_buf = BufWriter::with_capacity(cli_args.original.buffer_size, stream.send);

    // The filename in the protocol is the file part only of src_filename
    let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks
    let header = FileHeader::serialize_direct(meta.len(), protocol_filename);
    send_buf.write_all(&header).await?;

    // A server-side abort might happen part-way through a large transfer.
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

    let trailer = FileTrailer::serialize_direct();
    send_buf.write_all(&trailer).await?;
    send_buf.flush().await?;

    let response = Response::read(&mut stream.recv).await?;
    if response.status != Status::Ok {
        anyhow::bail!(format!(
            "PUT ({src_filename}) failed on completion check: {response}"
        ));
    }

    trace!("complete");
    Ok(())
}
