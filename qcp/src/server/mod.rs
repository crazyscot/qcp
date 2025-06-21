//! server-side _(remote)_ event loop
// (c) 2024 Ross Younger

use crate::cli::styles::use_colours;
use crate::config::Manager;

use anyhow::Context as _;
use human_repr::HumanDuration;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};

mod connection;
use connection::handle_connection;
mod connection_info;
use connection_info::parse_ssh_env;
mod stream;
use stream::handle_stream;

/// Server event loop
#[allow(clippy::module_name_repetitions)]
pub(crate) async fn server_main() -> anyhow::Result<()> {
    let mut control = crate::control::stdio_channel();
    let env_ssh_connection = std::env::var("SSH_CONNECTION").ok();
    let env_ssh_client = std::env::var("SSH_CLIENT").ok();
    let remote_ip = parse_ssh_env(env_ssh_connection.as_deref(), env_ssh_client.as_deref());
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
