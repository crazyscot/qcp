//! server-side _(remote)_ event loop
// (c) 2024 Ross Younger

use crate::cli::styles::use_colours;
use crate::config::Manager;
use crate::protocol::common::{ProtocolMessage as _, SendReceivePair};
use crate::protocol::session::Command;

use anyhow::Context as _;
use human_repr::HumanDuration;
use quinn::ConnectionStats;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{Instrument, debug, error, info, trace, trace_span, warn};

/// Server event loop
#[allow(clippy::module_name_repetitions)]
pub(crate) async fn server_main() -> anyhow::Result<()> {
    let mut control = crate::control::stdio_channel();
    let remote_ip = ssh_remote_address();
    let mut manager = Manager::standard(remote_ip.as_deref());
    let result = control
        .run_server(
            remote_ip,
            &mut manager,
            crate::util::setup_tracing,
            use_colours(),
        )
        .await?;
    let endpoint = result.endpoint;

    let mut tasks = JoinSet::new();

    // Main loop:
    // Wait for a successful connection OR timeout OR for stdin to be closed (implicitly handled).
    // We have tight control over what we expect (TLS peer certificate/name) so only need to handle one successful connection,
    // but a timeout is useful to give the user a cue that UDP isn't getting there.
    trace!("waiting for QUIC");
    let (stats_tx, mut stats_rx) = oneshot::channel();
    if let Some(conn) = timeout(result.config.timeout_duration(), endpoint.accept())
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

    debug!(
        "Remote stats: final mtu={pmtu}, rtt={rtt}",
        pmtu = stats.path.current_mtu,
        rtt = stats.path.rtt.human_duration()
    );
    control.send_closedown_report(&stats).await?;
    trace!("finished");
    Ok(())
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
