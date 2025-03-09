//! Main client mode event loop
// (c) 2024 Ross Younger

use crate::{
    client::{control::Channel, progress::spinner_style},
    config::{Configuration, Configuration_Optional, Manager},
    transport::ThroughputMode,
    util::{
        self, Credentials, TimeFormat, lookup_host_by_family,
        time::{Stopwatch, StopwatchChain},
    },
};

use anyhow::{Context, Result};
use futures_util::TryFutureExt as _;
use indicatif::{MultiProgress, ProgressBar};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{Connection, EndpointConfig, rustls};
use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::Arc;
use tokio::{self, time::Duration, time::timeout};
use tracing::{Instrument as _, Level, debug, error, info, span, trace, trace_span, warn};

use super::Parameters as ClientParameters;
use super::job::CopyJobSpec;

/// a shared definition string used in a couple of places
const SHOW_TIME: &str = "file transfer";

fn setup_tracing(
    display: &MultiProgress,
    parameters: &ClientParameters,
    time_format: TimeFormat,
) -> anyhow::Result<()> {
    util::setup_tracing(
        util::trace_level(parameters),
        Some(display),
        &parameters.log_file,
        time_format,
    ) // to provoke error: set RUST_LOG=.
}

/// Main client mode event loop
///
/// # Return value
/// `true` if the requested operation succeeded.
///
// Caution: As we are using ProgressBar, anything to be printed to console should use progress.println() !
#[allow(clippy::module_name_repetitions)]
#[allow(clippy::too_many_lines)]
pub async fn client_main(
    manager: &mut Manager,
    display: MultiProgress,
    parameters: ClientParameters,
) -> anyhow::Result<bool> {
    let working_config = manager.get::<Configuration_Optional>().unwrap_or_default();
    setup_tracing(
        &display,
        &parameters,
        working_config.time_format.unwrap_or_default(),
    )?;
    let default_config = Configuration::system_default();

    // N.B. While we have a MultiProgress we do not set up any `ProgressBar` within it yet...
    // not until the control channel is in place, in case ssh wants to ask for a password or passphrase.
    let _guard = trace_span!("CLIENT").entered();
    let mut timers = StopwatchChain::new_running("setup");

    let spinner = if parameters.quiet {
        ProgressBar::hidden()
    } else {
        display.add(ProgressBar::new_spinner().with_style(spinner_style()?))
    };
    spinner.enable_steady_tick(Duration::from_millis(150));

    // Prep --------------------------
    spinner.set_message("Preparing");
    let job_spec = crate::client::CopyJobSpec::try_from(&parameters)?;
    let credentials = Credentials::generate()?;
    let remote_ssh_hostname = job_spec.remote_host();
    let remote_dns_name = super::ssh::resolve_host_alias(
        remote_ssh_hostname,
        &working_config
            .ssh_config
            .unwrap_or(default_config.ssh_config.clone()),
    )
    .unwrap_or_else(|| remote_ssh_hostname.into());

    // If the user didn't specify the address family: we do the DNS lookup, figure it out and tell ssh to use that.
    // (Otherwise if we resolved a v4 and ssh a v6 - as might happen with round-robin DNS - that could be surprising.)
    let remote_address = lookup_host_by_family(
        &remote_dns_name,
        working_config
            .address_family
            .unwrap_or(default_config.address_family),
    )?;

    // Control channel ---------------
    spinner.set_message("Opening control channel");
    spinner.disable_steady_tick(); // otherwise the spinner messes with ssh passphrase prompting; as we're using tokio spinner.suspend() isn't helpful
    timers.next("control channel");
    let mut control = Channel::transact(
        &credentials,
        remote_ssh_hostname,
        remote_address.into(),
        &display,
        manager,
        &parameters,
    )
    .await?;
    let port = control.message.port;
    let config = manager
        .get::<Configuration>()
        .context("assembling final client configuration from server message")?;

    // Dry run mode ends here! -------
    if parameters.dry_run {
        info!("Dry run mode selected, not connecting to data channel");
        info!(
            "Negotiated network configuration: {}",
            config.format_transport_config()
        );
        return Ok(true);
    }

    // Data channel ------------------
    let server_address_port = match remote_address {
        std::net::IpAddr::V4(ip) => SocketAddrV4::new(ip, port).into(),
        std::net::IpAddr::V6(ip) => SocketAddrV6::new(ip, port, 0, 0).into(),
    };

    spinner.enable_steady_tick(Duration::from_millis(150));
    spinner.set_message("Establishing data channel");
    timers.next("data channel setup");
    let endpoint = create_endpoint(
        &credentials,
        control.message.cert.clone().into(),
        &server_address_port,
        &config,
        job_spec.throughput_mode(),
    )?;

    debug!("Opening QUIC connection to {server_address_port:?}");
    debug!("Local endpoint address is {:?}", endpoint.local_addr()?);
    let connection = timeout(
        config.timeout_duration(),
        endpoint.connect(server_address_port, &control.message.name)?,
    )
    .await
    .context("UDP connection to QUIC endpoint timed out")??;

    // Show time! ---------------------
    spinner.set_message("Transferring data");
    timers.next(SHOW_TIME);
    let result = manage_request(
        &connection,
        job_spec,
        display.clone(),
        spinner.clone(),
        &config,
        parameters.quiet,
    )
    .await;
    let total_bytes = match result {
        Err(b) | Ok(b) => b,
    };

    // Closedown ----------------------
    timers.next("shutdown");
    spinner.set_message("Shutting down");
    // Forcibly (but gracefully) tear down QUIC. All the requests have completed or errored.
    endpoint.close(1u8.into(), "finished".as_bytes());
    let remote_stats = control.read_closedown_report().await?;

    let control_fut = control.close();
    let _ = timeout(config.timeout_duration(), endpoint.wait_idle())
        .await
        .inspect_err(|_| warn!("QUIC shutdown timed out")); // otherwise ignore errors
    trace!("QUIC closed; waiting for control channel");
    let _ = timeout(config.timeout_duration(), control_fut)
        .await
        .inspect_err(|_| warn!("control channel timed out"));
    // Ignore errors. If the control channel closedown times out, we expect its drop handler will do the Right Thing.

    timers.stop();

    // Post-transfer chatter -----------
    if !parameters.quiet {
        let transport_time = timers.find(SHOW_TIME).and_then(Stopwatch::elapsed);
        crate::util::stats::process_statistics(
            &connection.stats(),
            total_bytes,
            transport_time,
            remote_stats,
            &config,
            parameters.statistics,
        );
    }

    if parameters.profile {
        info!("Elapsed time by phase:\n{timers}");
    }
    display.clear()?;
    Ok(result.is_ok())
}

/// Do whatever it is we were asked to.
/// On success: returns the number of bytes transferred.
/// On error: returns the number of bytes that were transferred, as far as we know.
async fn manage_request(
    connection: &Connection,
    copy_spec: CopyJobSpec,
    display: MultiProgress,
    spinner: ProgressBar,
    config: &Configuration,
    quiet: bool,
) -> Result<u64, u64> {
    let mut tasks = tokio::task::JoinSet::new();
    let connection = connection.clone();
    let config = config.clone();
    let _jh = tasks.spawn(async move {
        use crate::session;

        // This async block returns a Result<u64>
        let sp = connection.open_bi().map_err(|e| anyhow::anyhow!(e)).await?;
        // Called function returns its payload size.
        // This async block reports on errors.

        if copy_spec.source.user_at_host.is_some() {
            let mut imp = session::Get::boxed(sp.into(), None);
            imp.send(&copy_spec, display, spinner, &config, quiet)
                .instrument(trace_span!("GET", filename = copy_spec.source.filename))
                .await
        } else {
            let mut imp = session::Put::boxed(sp.into(), None);
            imp.send(&copy_spec, display, spinner, &config, quiet)
                .instrument(trace_span!("PUT", filename = copy_spec.source.filename))
                .await
        }
    });

    let mut total_bytes = 0u64;
    let mut success = true;
    loop {
        let Some(result) = tasks.join_next().await else {
            break;
        };
        // The first layer of possible errors are Join errors
        let result = match result {
            Ok(r) => r,
            Err(err) => {
                // This is either a panic, or a cancellation.
                if let Ok(reason) = err.try_into_panic() {
                    // Resume the panic on the main task
                    std::panic::resume_unwind(reason);
                }
                warn!("unexpected task join failure (shouldn't happen)");
                Ok(0)
            }
        };

        // The second layer of possible errors are failures in the protocol. Continue with other jobs as far as possible.
        match result {
            Ok(size) => total_bytes += size,
            Err(e) => {
                error!("{e}");
                success = false;
            }
        }
    }
    if success {
        Ok(total_bytes)
    } else {
        Err(total_bytes)
    }
}

/// Creates the client endpoint:
/// `credentials` are generated locally.
/// `server_cert` comes from the control channel server message.
/// `destination` is the server's address (port from the control channel server message).
pub(crate) fn create_endpoint(
    credentials: &Credentials,
    server_cert: CertificateDer<'_>,
    server_addr: &SocketAddr,
    options: &Configuration,
    mode: ThroughputMode,
) -> Result<quinn::Endpoint> {
    let _ = span!(Level::TRACE, "create_endpoint").entered();
    let mut root_store = RootCertStore::empty();
    root_store.add(server_cert)?;

    let tls_config = Arc::new(
        rustls::ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_client_auth_cert(credentials.cert_chain(), credentials.keypair.clone_key())?,
    );

    let mut config = quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls_config)?));
    let _ = config.transport_config(crate::transport::create_config(options, mode)?);

    trace!("bind & configure socket, port={:?}", options.port);
    let mut socket = util::socket::bind_range_for_peer(server_addr, options.port)?;
    let wanted_send = match mode {
        ThroughputMode::Both | ThroughputMode::Tx => Some(Configuration::send_buffer().try_into()?),
        ThroughputMode::Rx => None,
    };
    let wanted_recv = match mode {
        ThroughputMode::Both | ThroughputMode::Rx => Some(Configuration::recv_buffer().try_into()?),
        ThroughputMode::Tx => None,
    };

    if let Some(msg) = util::socket::set_udp_buffer_sizes(&mut socket, wanted_send, wanted_recv)? {
        warn!("{msg}");
    }

    trace!("create endpoint");
    // SOMEDAY: allow user to specify max_udp_payload_size in endpoint config, to support jumbo frames
    let runtime =
        quinn::default_runtime().ok_or_else(|| anyhow::anyhow!("no async runtime found"))?;
    let mut endpoint = quinn::Endpoint::new(EndpointConfig::default(), None, socket, runtime)?;
    endpoint.set_default_client_config(config);

    Ok(endpoint)
}
