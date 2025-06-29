//! server-side _(remote)_ event loop
// (c) 2024 Ross Younger

use crate::cli::styles::use_colours;
use crate::config::Manager;
use crate::control::ControlChannelServerInterface;
use crate::protocol::common::{ReceivingStream, SendingStream};
use crate::util::setup_tracing;

use anyhow::Context as _;
use human_repr::HumanDuration;
use tokio::sync::oneshot;
use tokio::task::JoinSet;
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};

mod connection;
mod connection_info;
use connection_info::parse_ssh_env;
mod stream;
use stream::handle_stream;

/// Server event loop
#[allow(clippy::module_name_repetitions)]
#[cfg_attr(coverage_nightly, coverage(off))] // This is a thin adaptor, not worth testing
pub(crate) async fn server_main() -> anyhow::Result<()> {
    let control = crate::control::stdio_channel();
    let env_ssh_connection = std::env::var("SSH_CONNECTION").ok();
    let env_ssh_client = std::env::var("SSH_CLIENT").ok();
    let remote_ip = parse_ssh_env(env_ssh_connection.as_deref(), env_ssh_client.as_deref());
    let mut manager = Manager::standard(remote_ip.as_deref());

    server_main_inner(control, remote_ip, &mut manager).await
}

/// Server event loop with dependency injection for unit tests
#[allow(clippy::module_name_repetitions)]
async fn server_main_inner<
    S: SendingStream + 'static,
    R: ReceivingStream + 'static,
    CC: ControlChannelServerInterface<S, R>,
>(
    mut control: CC,
    remote_ip: Option<String>,
    manager: &mut Manager,
) -> anyhow::Result<()> {
    let result = control
        .run_server(remote_ip, manager, setup_tracing, use_colours())
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
            let result = connection::handle_incoming(conn).await;
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use std::net::UdpSocket;

    use crate::Configuration;
    use crate::config::Manager;
    use crate::control::{MockControlChannelServerInterface, ServerResult};
    use crate::server::server_main_inner;

    use quinn::{Endpoint, EndpointConfig};
    use tokio_test::io::Mock as MockStream;

    //use mockall::predicate::*;
    use mockall::*;

    // channel.rs already has mocks and Control.run_server() is already tested.
    // For this test, we only really care that the main loop in this unit does something sensible.

    #[cfg_attr(cross_target_mingw, ignore)]
    // TODO: Cross-compiled mingw code fails here in quinn::Endpoint::new
    // with Endpoint Failed: OS Error 10045 (FormatMessageW() returned error 317) (os error 10045)
    // Don't run this test on such cross builds for now.
    #[tokio::test]
    async fn control_channel_basic() {
        let mut manager = Manager::standard(None);
        manager.apply_system_default();
        let expected_config = manager.get::<Configuration>().unwrap();
        let hostname = "myserver";

        let mut mock_control = MockControlChannelServerInterface::<MockStream, MockStream>::new();
        let _expect = mock_control
            .expect_run_server()
            .with(
                predicate::eq(Some(hostname.into())),
                predicate::function(move |mgr: &Manager| {
                    mgr.get::<Configuration>().unwrap() == expected_config
                }),
                predicate::always(),
                predicate::always(),
            )
            .times(1)
            .returning(|_ip, mgr, _setup_tracing, _colour| {
                let runtime = quinn::default_runtime().unwrap();

                let endpoint = Endpoint::new(
                    EndpointConfig::default(),
                    None,
                    UdpSocket::bind("127.0.0.1:0").unwrap(),
                    runtime,
                )
                .unwrap();
                // This isn't currently a mocked Endpoint, so all we can really do is cause the server loop to exit.
                endpoint.close(0u8.into(), &[]);

                Ok(ServerResult {
                    config: mgr.get::<Configuration>().unwrap(),
                    endpoint,
                })
            });
        let _expect = mock_control
            .expect_send_closedown_report()
            .with(predicate::always())
            .times(1)
            .returning(|_| Ok(()));

        server_main_inner(mock_control, Some(hostname.into()), &mut manager)
            .await
            .unwrap();
    }
}
