// qcp client event loop
// (c) 2024 Ross Younger

use crate::client::args::ProcessedArgs;
use crate::protocol::control::{control_capnp, ServerMessage};
use crate::protocol::session::session_capnp::Status;
use crate::protocol::session::{session_capnp, FileHeader, FileTrailer, Response};
use crate::protocol::{RawStreamPair, StreamPair};
use crate::{cert::Credentials, protocol};

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
use std::str::FromStr as _;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::{self, io::AsyncReadExt, time::timeout, time::Duration};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, span, trace, trace_span, warn, Level};

use super::ClientArgs;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(5);

/// Main CLI entrypoint
#[tokio::main]
pub async fn client_main(args: &ClientArgs) -> anyhow::Result<bool> {
    let unpacked_args = ProcessedArgs::try_from(args)?;
    println!("{unpacked_args:?}"); // TEMP

    let span = trace_span!("CLIENT");
    let _guard = span.enter();
    let credentials = crate::cert::Credentials::generate()?;

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

    let host = unpacked_args.remote_host();
    let server_address = async_dns::lookup(host)
        .await
        .inspect_err(|e| {
            error!("host name lookup for {host} failed: {e}");
        })?
        .next()
        .ok_or_else(|| {
            error!("host name lookup failed2");
            anyhow::anyhow!("host name lookup failed")
        })?;

    let server_address_port = match server_address.ip_address {
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
    // bring down QUIC gracefully
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

    // Hard wire a single GET for now.
    // TODO this will spawn ? or setup multiple futures and await all ?
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
        do_get(sp, &args.source.filename, &args.destination.filename).await
    } else {
        todo!();
        // do_put(sp, &args.source.filename, &args.destination.filename).await
    }
}

fn launch_server(args: &ProcessedArgs) -> Result<Child> {
    let remote_host = args.remote_host();
    let mut server = Command::new("ssh");
    // TODO extra ssh options
    server.args([remote_host, "qcpt"]);
    if args.original.debug {
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

async fn do_get(sp: RawStreamPair, filename: &str, dest: &str) -> Result<()> {
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
    capnp_futures::serialize::write_message(&mut stream.send, msg).await?;

    // TODO protocol timeout?
    trace!("await response");
    let response = Response::read(&mut stream.recv).await?;
    trace!("GET {response:?}");

    if response.status != Status::Ok {
        let msg_msg = match response.message {
            Some(s) => format!("with message {s}"),
            None => "".to_string(),
        };
        anyhow::bail!(format!(
            "GET {filename} failed: server returned status {status:?} {msg_msg}",
            status = response.status
        ));
    }

    let header = FileHeader::read(&mut stream.recv).await?;
    trace!("GET: HEADER {header:?}");

    let mut dest_path = PathBuf::from_str(dest).unwrap();
    let dest_meta = tokio::fs::metadata(&dest_path).await;
    if let Ok(meta) = dest_meta {
        // if it's a file, proceed (overwriting)
        if meta.is_dir() {
            dest_path.push(header.filename);
        } else if meta.is_symlink() {
            // TODO: Need to cope with this case; test whether it's a directory?
            let deref = std::fs::read_link(&dest_path)?;
            if std::fs::metadata(deref).is_ok_and(|meta| meta.is_dir()) {
                dest_path.push(header.filename);
            }
            // Else assume the link points to a file, which we will overwrite.
        }
    }

    let mut file = tokio::fs::File::create(dest_path).await?;

    let mut read_n = stream.recv.get_mut().take(header.size);
    tokio::io::copy(&mut read_n, &mut file).await?;

    let _trailer = FileTrailer::read(&mut stream.recv).await?;
    // Trailer is empty for now, but its existence means the server believes the file was sent correctly

    Ok(())
}
