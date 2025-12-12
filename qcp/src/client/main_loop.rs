//! Main client mode event loop
// (c) 2024 Ross Younger

use crate::{
    cli::styles::use_colours,
    client::progress::SPINNER_TEMPLATE,
    config::{Configuration, Configuration_Optional, Manager},
    control::{ControlChannel, create, create_endpoint},
    protocol::{
        TaggedData,
        common::{ReceivingStream, SendReceivePair, SendingStream},
        compat::Feature,
        control::{ClosedownReportV1, Compatibility, CredentialsType, Direction, ServerMessageV2},
        session::{CommandParam, Get2Args},
    },
    session::CommandStats,
    util::{
        self, Credentials, lookup_host_by_family,
        process::ProcessWrapper,
        time::{Stopwatch, StopwatchChain},
    },
};

use anyhow::{Context, Result};
use futures_util::TryFutureExt as _;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use quinn::{Connection as QuinnConnection, Endpoint};
use std::{
    future::Future,
    net::{IpAddr, SocketAddr, SocketAddrV4, SocketAddrV6},
};
use tokio::{
    self,
    process::{ChildStdin, ChildStdout},
    time::{Duration, timeout},
};
use tracing::{Instrument as _, debug, error, info, trace, trace_span, warn};

use super::Parameters as ClientParameters;
use super::job::CopyJobSpec;

/// a shared definition string used in a couple of places
const SHOW_TIME: &str = "file transfer";

/// Main client mode event loop
///
/// # Return value
/// `true` if the requested operation succeeded.
///
// Caution: As we are using ProgressBar, anything to be printed to console should use progress.println() !
pub(crate) async fn client_main(
    manager: Manager,
    display: MultiProgress,
    parameters: ClientParameters,
) -> anyhow::Result<bool> {
    Client::new(manager, display, parameters)?.run().await
}

#[derive(Default, Debug, derive_more::Constructor)]
struct RequestResult {
    success: bool,
    stats: CommandStats,
}

struct Client {
    manager: Manager,
    display: MultiProgress,
    parameters: ClientParameters,
    credentials: Credentials,
    timers: StopwatchChain,
    spinner: ProgressBar,
}

#[derive(Debug, PartialEq)]
struct PrepResult {
    remote_address: IpAddr,
    job_specs: Vec<CopyJobSpec>,
}

impl PrepResult {
    fn primary_job(&self) -> &CopyJobSpec {
        self.job_specs
            .first()
            .expect("prep should always produce at least one job")
    }

    fn remote_host(&self) -> &str {
        self.primary_job().remote_host()
    }

    fn direction(&self) -> Direction {
        self.primary_job().direction()
    }

    fn preserve(&self) -> bool {
        self.primary_job().preserve
    }
}

type ControlChannelType = ControlChannel<ChildStdin, ChildStdout>;

struct QcpConnection {
    ssh_client: ProcessWrapper,
    control: ControlChannelType,
    endpoint: Option<Endpoint>,
    server_message: ServerMessageV2,
}

impl TryFrom<ProcessWrapper> for QcpConnection {
    type Error = anyhow::Error;
    fn try_from(mut client: ProcessWrapper) -> Result<Self> {
        let control = ControlChannel::new(client.stream_pair()?);
        Ok(Self {
            ssh_client: client,
            control,
            server_message: ServerMessageV2::default(),
            endpoint: None,
        })
    }
}

impl Client {
    fn new(manager: Manager, display: MultiProgress, parameters: ClientParameters) -> Result<Self> {
        let spinner = if parameters.quiet {
            ProgressBar::hidden()
        } else {
            display.add(
                ProgressBar::new_spinner()
                    .with_style(ProgressStyle::with_template(SPINNER_TEMPLATE)?),
            )
        };

        Ok(Self {
            manager,
            display,
            parameters,
            credentials: Credentials::generate()?,
            timers: StopwatchChain::default(),
            spinner,
        })
    }

    /// Main client mode event loop
    ///
    /// # Return value
    /// `true` if the requested operation succeeded.
    ///
    // Caution: As we are using ProgressBar, anything to be printed to console should use progress.println() !
    pub(crate) async fn run(&mut self) -> anyhow::Result<bool> {
        self.timers.next("Setup");

        let working_config = self
            .manager
            .get::<Configuration_Optional>()
            .unwrap_or_default();

        util::setup_tracing(
            util::trace_level(&self.parameters),
            util::ConsoleTraceType::Indicatif(self.display.clone()),
            self.parameters.log_file.as_ref(),
            working_config.time_format.unwrap_or_default(),
            use_colours(),
        )?; // to provoke error: set RUST_LOG=.

        let default_config = Configuration::system_default();

        let prep_result = {
            let _prep_span = trace_span!("Prep").entered();
            self.prep(&working_config, default_config)?
        };

        // Control channel ---------------
        let (config, mut qcp_conn) = self
            .establish_control_channel(&working_config, &prep_result)
            .await
            .context("while establishing control channel")?;

        // Dry run mode ends here! -------
        if self.parameters.dry_run {
            info!("Dry run mode selected, not connecting to data channel");
            info!(
                "Negotiated network configuration: {}",
                config.format_transport_config()
            );
            return Ok(true);
        }

        // Data channel ------------------

        let connection = self
            .establish_data_channel(&prep_result, &config, &mut qcp_conn)
            .await?;

        // Show time! ---------------------

        self.spinner.set_message("Transferring data");
        self.timers.next(SHOW_TIME);
        let quiet = self.parameters.quiet;
        let compat = qcp_conn.control.selected_compat;
        let (overall_success, aggregate_stats) = process_job_requests(
            &prep_result.job_specs,
            || async {
                let bi = connection.open_bi().map_err(|e| anyhow::anyhow!(e)).await?;
                Ok(SendReceivePair::from(bi))
            },
            |stream_pair, job| self.manage_request(stream_pair, job, &config, quiet, compat),
        )
        .await?;

        // Closedown ----------------------
        let remote_stats = self.closedown(&config, qcp_conn).await?;

        // Post-transfer chatter -----------
        if !self.parameters.quiet {
            let transport_time = self.timers.find(SHOW_TIME).and_then(Stopwatch::elapsed);
            crate::util::stats::process_statistics(
                &connection.stats(),
                aggregate_stats,
                transport_time,
                &remote_stats,
                &config,
                self.parameters.statistics,
            );
        }

        if self.parameters.profile {
            info!("Elapsed time by phase:\n{}", self.timers);
        }
        self.display.clear()?;
        Ok(overall_success)
    }

    pub(crate) fn prep(
        &mut self,
        working_config: &Configuration_Optional,
        default_config: &Configuration,
    ) -> anyhow::Result<PrepResult> {
        // N.B. While we have a MultiProgress we do not set up a `ProgressBar` within it yet
        // (spinners are OK though)...
        // not until the control channel is in place, in case ssh wants to ask for a password or passphrase.

        self.spinner.set_message("Preparing");
        self.spinner.enable_steady_tick(Duration::from_millis(150));

        let job_specs: Vec<CopyJobSpec> = (&self.parameters).try_into()?;
        let remote_ssh_hostname = job_specs
            .first()
            .expect("at least one job spec is required")
            .remote_host();

        let ssh_config_files = super::ssh::SshConfigFiles::new(
            working_config
                .ssh_config
                .as_ref()
                .unwrap_or(&default_config.ssh_config)
                .as_ref(),
        );
        let remote_dns_name = ssh_config_files
            .resolve_host_alias(remote_ssh_hostname)
            .unwrap_or_else(|| remote_ssh_hostname.to_string());

        // If the user didn't specify the address family: we do the DNS lookup, figure it out and tell ssh to use that.
        // (Otherwise if we resolved a v4 and ssh a v6 - as might happen with round-robin DNS - that could be surprising.)
        let remote_address = lookup_host_by_family(
            &remote_dns_name,
            working_config
                .address_family
                .unwrap_or(default_config.address_family),
        )?;
        Ok(PrepResult {
            remote_address,
            job_specs,
        })
    }

    async fn establish_control_channel(
        &mut self,
        working_config: &Configuration_Optional,
        prep_result: &PrepResult,
    ) -> anyhow::Result<(Configuration, QcpConnection)> {
        self.spinner.set_message("Opening control channel");
        self.spinner.disable_steady_tick(); // otherwise the spinner messes with ssh passphrase prompting; as we're using tokio spinner.suspend() isn't helpful
        self.timers.next("control channel");

        let ssh_client = create(
            &self.display,
            working_config,
            &self.parameters,
            prep_result.remote_host(),
            prep_result.remote_address.into(),
        )?;
        let mut qcp_conn = QcpConnection::try_from(ssh_client)?;

        qcp_conn.server_message = qcp_conn
            .control
            .run_client(
                &self.credentials,
                prep_result.remote_address.into(),
                &mut self.manager,
                &self.parameters,
                prep_result.direction(),
                None,
            )
            .await?;

        let config = self
            .manager
            .get::<Configuration>()
            .context("assembling final client configuration from server message")?;

        // Are any warnings necessary?
        if prep_result.preserve() && !qcp_conn.control.selected_compat.supports(Feature::PRESERVE) {
            warn!("--preserve requested, but remote does not support this option");
        }
        Ok((config, qcp_conn))
    }

    async fn establish_data_channel(
        &mut self,
        prep_result: &PrepResult,
        config: &Configuration,
        qcp_conn: &mut QcpConnection,
    ) -> anyhow::Result<QuinnConnection> {
        let message1 = &qcp_conn.server_message;
        let server_address_port = match prep_result.remote_address {
            std::net::IpAddr::V4(ip) => SocketAddrV4::new(ip, message1.port).into(),
            std::net::IpAddr::V6(ip) => SocketAddrV6::new(ip, message1.port, 0, 0).into(),
        };

        let endpoint = self.create_quic_endpoint(
            prep_result,
            config,
            &message1.credentials,
            server_address_port,
            qcp_conn.control.selected_compat,
        )?;

        debug!("Opening QUIC connection to {server_address_port:?}");
        let connection = timeout(
            config.timeout_duration(),
            endpoint.connect(server_address_port, &message1.common_name)?,
        )
        .await
        .context("UDP connection to QUIC endpoint timed out")??;
        qcp_conn.endpoint = Some(endpoint);
        Ok(connection)
    }

    fn create_quic_endpoint(
        &mut self,
        prep_result: &PrepResult,
        config: &Configuration,
        peer_credentials: &TaggedData<CredentialsType>,
        server_address_port: SocketAddr,
        compat: Compatibility,
    ) -> anyhow::Result<Endpoint> {
        self.spinner.enable_steady_tick(Duration::from_millis(150));
        self.spinner.set_message("Establishing data channel");
        self.timers.next("data channel setup");
        let (endpoint, _) = create_endpoint(
            &self.credentials,
            peer_credentials,
            server_address_port.into(),
            config,
            prep_result.direction().client_mode(),
            false,
            compat,
        )?;
        debug!("Local endpoint address is {:?}", endpoint.local_addr()?);
        Ok(endpoint)
    }

    async fn closedown(
        &mut self,
        config: &Configuration,
        mut conn: QcpConnection, // ctrl_result is consumed
    ) -> anyhow::Result<ClosedownReportV1> {
        self.timers.next("shutdown");
        self.spinner.set_message("Shutting down");
        // Forcibly (but gracefully) tear down QUIC. All the requests have completed or errored.
        let endpoint = conn.endpoint.take();
        if let Some(ref ep) = endpoint {
            trace!("Closing QUIC endpoint");
            ep.close(0u32.into(), "finished".as_bytes());
        }
        let remote_stats = conn.control.read_closedown_report().await?;

        let control_fut = conn.ssh_client.close();
        if let Some(ep) = endpoint {
            let _ = timeout(config.timeout_duration(), ep.wait_idle())
                .await
                .inspect_err(|_| warn!("QUIC shutdown timed out")); // otherwise ignore errors
        }
        trace!("QUIC closed; waiting for control channel");
        let _ = timeout(config.timeout_duration(), control_fut)
            .await
            .inspect_err(|_| warn!("control channel timed out"));
        // Ignore errors. If the control channel closedown times out, we expect its drop handler will do the Right Thing.

        self.timers.stop();

        Ok(remote_stats)
    }

    /// Do whatever it is we were asked to.
    /// On success: returns statistics about the transfer.
    /// On error: returns the transfer statistics, as far as we know, up to the point of failure
    async fn manage_request<S, R>(
        &self,
        stream_pair: SendReceivePair<S, R>,
        copy_spec: CopyJobSpec,
        config: &Configuration,
        quiet: bool,
        compat: Compatibility,
    ) -> RequestResult
    where
        S: SendingStream + 'static,
        R: ReceivingStream + 'static,
    {
        use crate::session;

        let display = self.display.clone();
        let spinner = self.spinner.clone();

        let (mut handler, span) = if copy_spec.source.user_at_host.is_some() {
            let mut args = Get2Args::default();
            if copy_spec.preserve {
                args.options.push(CommandParam::PreserveMetadata.into());
            }
            (
                session::Get::boxed(stream_pair, Some(args), compat),
                trace_span!("GETx", filename = copy_spec.source.filename.clone()),
            )
        } else {
            (
                session::Put::boxed(stream_pair, None, compat),
                trace_span!("PUTx", filename = copy_spec.source.filename.clone()),
            )
        };
        let result = handler
            .send(&copy_spec, display, spinner, config, quiet)
            .instrument(span)
            .await;

        match result {
            Ok(st) => RequestResult::new(true, st),
            Err(e) => {
                if let Some(src) = e.source() {
                    // Some error conditions come with an anyhow Context.
                    // We want to output one tidy line, so glue them together.
                    error!("{e}: {src}");
                } else {
                    error!("{e}");
                }
                RequestResult::new(false, CommandStats::new())
            }
        }
    }
}

async fn process_job_requests<S, R, OpenStream, OpenFut, HandleJob, HandleFut>(
    jobs: &[CopyJobSpec],
    mut open_stream: OpenStream,
    mut handle_job: HandleJob,
) -> anyhow::Result<(bool, CommandStats)>
where
    OpenStream: FnMut() -> OpenFut,
    OpenFut: Future<Output = anyhow::Result<SendReceivePair<S, R>>>,
    HandleJob: FnMut(SendReceivePair<S, R>, CopyJobSpec) -> HandleFut,
    HandleFut: Future<Output = RequestResult>,
    S: SendingStream + 'static,
    R: ReceivingStream + 'static,
{
    let mut aggregate_stats = CommandStats::new();
    let mut overall_success = true;
    for job in jobs {
        let stream_pair = open_stream().await?;
        let result = handle_job(stream_pair, job.clone()).await;

        aggregate_stats.payload_bytes += result.stats.payload_bytes;
        aggregate_stats.peak_transfer_rate = aggregate_stats
            .peak_transfer_rate
            .max(result.stats.peak_transfer_rate);
        if !result.success {
            overall_success = false;
            break;
        }
    }
    Ok((overall_success, aggregate_stats))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use indicatif::MultiProgress;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::{net::Ipv4Addr, str::FromStr};
    use tokio::io::AsyncWriteExt;

    use super::{RequestResult, process_job_requests};

    #[cfg(unix)]
    use crate::control::create_fake;

    use crate::session::CommandStats;
    use crate::{
        Configuration, CopyJobSpec, FileSpec, Parameters,
        client::main_loop::Client,
        config::{Configuration_Optional, Manager},
        protocol::{common::ProtocolMessage as _, test_helpers::new_test_plumbing},
    };

    fn make_uut<F: FnOnce(&mut Manager, &mut Parameters)>(f: F, src: &str, dest: &str) -> Client {
        let mut mgr = Manager::without_default(None);
        let mut params = Parameters {
            paths: vec![
                FileSpec::from_str(src).unwrap(),
                FileSpec::from_str(dest).unwrap(),
            ],
            ..Default::default()
        };
        f(&mut mgr, &mut params);

        Client::new(Manager::without_default(None), MultiProgress::new(), params).unwrap()
    }
    const REMOTE_FILE: &str = "8.8.8.8:file";
    const LOCAL_FILE: &str = "file";
    fn remote_file_spec() -> FileSpec {
        FileSpec::from_str(REMOTE_FILE).unwrap()
    }
    fn local_file_spec() -> FileSpec {
        FileSpec::from_str(LOCAL_FILE).unwrap()
    }

    #[test]
    fn prep_valid_hostname() {
        let mut uut = make_uut(|_, _| (), REMOTE_FILE, LOCAL_FILE);
        let working = Configuration_Optional::default();
        let res = uut.prep(&working, Configuration::system_default()).unwrap();
        assert_eq!(res.remote_address, Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(res.job_specs[0].source, remote_file_spec());
        assert_eq!(res.job_specs[0].destination, local_file_spec());
        eprintln!("{res:?}");
    }
    #[test]
    fn prep_invalid_hostname() {
        let mut uut = make_uut(|_, _| (), "no-such-host.invalid:file", "file");
        let working = Configuration_Optional::default();
        let _ = uut
            .prep(&working, Configuration::system_default())
            .unwrap_err();
    }

    #[cfg(unix)] // this test depends on create_fake, which is not implemented on Windows
    #[cfg_attr(target_os = "macos", ignore)]
    #[tokio::test]
    async fn endpoint_create_close() {
        use crate::client::main_loop::QcpConnection;
        use crate::protocol::control::{ClosedownReport, ClosedownReportV1, Compatibility};

        let mut uut = make_uut(|_, _| (), "127.0.0.1:file", LOCAL_FILE);
        let working = Configuration_Optional::default();
        let config = Configuration::system_default().clone();
        let prep_result = uut.prep(&working, Configuration::system_default()).unwrap();
        let server_cert = crate::util::Credentials::generate().unwrap();
        let server_address_port = (Ipv4Addr::LOCALHOST, 0);
        let level = Compatibility::Level(1);

        let endpoint = uut
            .create_quic_endpoint(
                &prep_result,
                &config,
                &server_cert.to_tagged_data(level, None).unwrap(),
                server_address_port.into(),
                level,
            )
            .unwrap();
        assert!(endpoint.local_addr().is_ok());

        let fake_report = ClosedownReport::V1(ClosedownReportV1::default());
        let mut buf = Vec::new();
        fake_report.to_writer_framed(&mut buf).unwrap();
        eprintln!("Fake report: {buf:?}");

        let ssh_client = create_fake(&buf);
        let mut qcp_conn = QcpConnection::try_from(ssh_client).unwrap();
        qcp_conn.endpoint = Some(endpoint);

        let report = uut.closedown(&config, qcp_conn).await.unwrap();
        assert_eq!(report, ClosedownReportV1::default());
        eprintln!("Closedown report: {report:?}");
    }

    #[tokio::test]
    async fn handle_get_succeeding() {
        use littertray::LitterTray;
        const TEST_DATA: &[u8] = b"test";
        let mut uut = make_uut(|_, _| (), "127.0.0.1:file", "outfile");
        let working = Configuration_Optional::default();
        let prep_result = uut.prep(&working, Configuration::system_default()).unwrap();
        let mut plumbing = new_test_plumbing();

        let manage_fut = uut.manage_request(
            plumbing.0,
            prep_result.job_specs[0].clone(),
            Configuration::system_default(),
            false,
            crate::protocol::control::Compatibility::Level(1),
        );

        // We are not really testing the protocol here, that is done in get.rs / put.rs.
        // But we are testing the main loop behaviour, so we need to send a valid response.
        // We will verify that the UUT created the file.
        // We need to send a Response, FileHeader, file data, then FileTrailer.
        let mut send_buf = Vec::new();
        crate::protocol::session::Response::V1(crate::protocol::session::ResponseV1 {
            status: crate::protocol::session::Status::Ok.into(),
            message: None,
        })
        .to_writer_framed(&mut send_buf)
        .unwrap();
        crate::protocol::session::FileHeader::new_v1(TEST_DATA.len() as u64, "outfile")
            .to_writer_framed(&mut send_buf)
            .unwrap();
        send_buf.extend_from_slice(TEST_DATA);
        crate::protocol::session::FileTrailer::V1
            .to_writer_framed(&mut send_buf)
            .unwrap();
        let send_fut = plumbing.1.send.write_all(&send_buf);

        // litter tray tidies up after the written file
        let r = LitterTray::try_with_async(async |_| {
            let (a, b) = tokio::join!(send_fut, manage_fut);
            let contents = std::fs::read("outfile")?;
            assert_eq!(contents, TEST_DATA);
            a.unwrap();
            Ok(b)
        })
        .await
        .unwrap();
        println!("Result: {r:?}");
        assert!(r.success);
        assert_eq!(r.stats.payload_bytes, TEST_DATA.len() as u64);
    }

    #[tokio::test]
    async fn handle_put_failing() {
        let mut uut = make_uut(|_, _| (), "/tmp/file", "127.0.0.1:file");
        let working = Configuration_Optional::default();
        let prep_result = uut.prep(&working, Configuration::system_default()).unwrap();
        let mut plumbing = new_test_plumbing();
        plumbing.1.send.shutdown().await.unwrap(); // this causes the handler to error out

        let manage_fut = uut.manage_request(
            plumbing.0,
            prep_result.job_specs[0].clone(),
            Configuration::system_default(),
            false,
            crate::protocol::control::Compatibility::Level(1),
        );
        let r = manage_fut.await;
        println!("Result: {r:?}");
    }

    #[tokio::test]
    async fn process_job_requests_aggregates_stats() {
        let jobs = vec![
            CopyJobSpec::from_parts("file1", "host:dir", false).unwrap(),
            CopyJobSpec::from_parts("file2", "host:dir", false).unwrap(),
        ];

        let open_calls = AtomicUsize::new(0);
        let handle_calls = AtomicUsize::new(0);
        let results = Mutex::new(vec![
            RequestResult::new(
                true,
                CommandStats {
                    payload_bytes: 10,
                    peak_transfer_rate: 100,
                },
            ),
            RequestResult::new(
                true,
                CommandStats {
                    payload_bytes: 5,
                    peak_transfer_rate: 200,
                },
            ),
        ]);

        let (success, stats) = process_job_requests(
            &jobs,
            || {
                let _ = open_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok::<_, anyhow::Error>(new_test_plumbing().0) }
            },
            |stream_pair, _job| {
                let _ = handle_calls.fetch_add(1, Ordering::SeqCst);
                drop(stream_pair);
                async { results.lock().unwrap().remove(0) }
            },
        )
        .await
        .unwrap();

        assert!(success);
        assert_eq!(open_calls.load(Ordering::SeqCst), 2);
        assert_eq!(handle_calls.load(Ordering::SeqCst), 2);
        assert_eq!(stats.payload_bytes, 15);
        assert_eq!(stats.peak_transfer_rate, 200);
    }

    #[tokio::test]
    async fn process_job_requests_stops_on_failure() {
        let jobs = vec![
            CopyJobSpec::from_parts("file1", "host:dir", false).unwrap(),
            CopyJobSpec::from_parts("file2", "host:dir", false).unwrap(),
            CopyJobSpec::from_parts("file3", "host:dir", false).unwrap(),
        ];

        let open_calls = AtomicUsize::new(0);
        let handle_calls = AtomicUsize::new(0);
        let results = Mutex::new(vec![
            RequestResult::new(
                true,
                CommandStats {
                    payload_bytes: 10,
                    peak_transfer_rate: 100,
                },
            ),
            RequestResult::new(false, CommandStats::new()),
            // This would only be consumed if we failed to stop early.
            RequestResult::new(
                true,
                CommandStats {
                    payload_bytes: 999,
                    peak_transfer_rate: 999,
                },
            ),
        ]);

        let (success, stats) = process_job_requests(
            &jobs,
            || {
                let _ = open_calls.fetch_add(1, Ordering::SeqCst);
                async { Ok::<_, anyhow::Error>(new_test_plumbing().0) }
            },
            |stream_pair, _job| {
                let _ = handle_calls.fetch_add(1, Ordering::SeqCst);
                drop(stream_pair);
                async { results.lock().unwrap().remove(0) }
            },
        )
        .await
        .unwrap();

        assert!(!success);
        assert_eq!(open_calls.load(Ordering::SeqCst), 2);
        assert_eq!(handle_calls.load(Ordering::SeqCst), 2);
        assert_eq!(stats.payload_bytes, 10);
        assert_eq!(stats.peak_transfer_rate, 100);
    }
}
