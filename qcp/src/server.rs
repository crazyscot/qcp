//! server-side _(remote)_ event loop
// (c) 2024 Ross Younger

use std::cmp::min;
use std::sync::Arc;

use crate::config::{Configuration, Configuration_Optional, Manager};

use crate::protocol::common::{ProtocolMessage as _, SendReceivePair};
use crate::protocol::control::{
    BANNER, COMPATIBILITY_LEVEL, ClientGreeting, ClientMessage, ClosedownReport, ClosedownReportV1,
    ConnectionType, ServerFailure, ServerGreeting, ServerMessage, ServerMessageV1,
};
use crate::protocol::session::Command;

use crate::transport::{ThroughputMode, combine_bandwidth_configurations};
use crate::util::{Credentials, TimeFormat, socket};

use anyhow::Context as _;
use quinn::crypto::rustls::QuicServerConfig;
use quinn::rustls::server::WebPkiClientVerifier;
use quinn::rustls::{self, RootCertStore};
use quinn::{ConnectionStats, EndpointConfig};
use rustls_pki_types::CertificateDer;
use serde_bare::Uint;
use tokio::io::AsyncWriteExt as _;
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
        initial_congestion_window: Uint(config.initial_congestion_window.into()),
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
            let result = handle_connection(conn).await;
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

async fn handle_connection(conn: quinn::Incoming) -> anyhow::Result<ConnectionStats> {
    let connection = conn.await?;
    debug!(
        "accepted QUIC connection from {}",
        connection.remote_address()
    );

    async {
        loop {
            let stream = connection.accept_bi().await;
            let sp = match stream {
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
                Ok(s) => SendReceivePair::from(s),
            };
            trace!("opened stream");
            let _j = tokio::spawn(async move {
                handle_stream(sp).await;
            });
        }
    }
    .await?;
    Ok(connection.stats())
}

async fn handle_stream(mut sp: SendReceivePair<quinn::SendStream, quinn::RecvStream>) {
    use crate::session;
    trace!("reading command");
    let packet = Command::from_reader_async_framed(&mut sp.recv).await;
    let Ok(cmd) = packet else {
        error!("failed to read command");
        return;
    };

    let (span, mut handler) = match cmd {
        Command::Get(args) => (
            trace_span!("SERVER:GET", filename = args.filename.clone()),
            session::Get::boxed(sp, Some(args)),
        ),
        Command::Put(args) => (
            trace_span!("SERVER:PUT", filename = args.filename.clone()),
            session::Put::boxed(sp, Some(args)),
        ),
    };
    if let Err(e) = handler.handle().instrument(span).await {
        error!("stream handler failed: {e}");
    }
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
