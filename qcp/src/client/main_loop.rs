//! Main client mode event loop
// (c) 2024 Ross Younger

use crate::{
    cli::{CliArgs, styles::use_colours},
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
        stats::format_rate,
        time::{Stopwatch, StopwatchChain},
    },
};

use anyhow::{Context, Result};
use async_trait::async_trait;
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
    args: Box<crate::cli::CliArgs>,
) -> anyhow::Result<bool> {
    Client::new(manager, display, args)?.run().await
}

#[derive(Default, Debug, derive_more::Constructor)]
struct RequestResult {
    success: bool,
    stats: CommandStats,
}

struct Client {
    manager: Manager,
    display: MultiProgress,
    credentials: Credentials,
    timers: StopwatchChain,
    spinner: ProgressBar,
    args: Box<CliArgs>,
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

#[async_trait]
trait BiStreamOpener {
    type Send: SendingStream + 'static;
    type Recv: ReceivingStream + 'static;

    async fn open_bi_stream(&self) -> Result<SendReceivePair<Self::Send, Self::Recv>>;
}

#[cfg_attr(coverage_nightly, coverage(off))] // thin adapter around quinn
#[async_trait]
impl BiStreamOpener for QuinnConnection {
    type Send = quinn::SendStream;
    type Recv = quinn::RecvStream;

    async fn open_bi_stream(&self) -> Result<SendReceivePair<Self::Send, Self::Recv>> {
        let bi = self.open_bi().await.map_err(|e| anyhow::anyhow!(e))?;
        Ok(SendReceivePair::from(bi))
    }
}

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
    fn new(manager: Manager, display: MultiProgress, args: Box<CliArgs>) -> Result<Self> {
        let spinner = if args.client_params.quiet {
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
            credentials: Credentials::generate()?,
            timers: StopwatchChain::default(),
            spinner,
            args,
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
            util::trace_level(&self.args.client_params),
            util::ConsoleTraceType::Indicatif(self.display.clone()),
            self.args.log_file.as_ref(),
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
        if self.args.client_params.dry_run {
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

        let direction = prep_result.direction();
        self.spinner.set_message("Transferring data");
        self.timers.next(SHOW_TIME);
        let (overall_success, aggregate_stats) = self
            .transfer_jobs(
                &connection,
                &prep_result.job_specs,
                &config,
                qcp_conn.control.selected_compat,
            )
            .await?;

        // Closedown ----------------------
        let remote_stats = self.closedown(&config, qcp_conn).await?;

        // Post-transfer chatter -----------
        if !self.args.client_params.quiet {
            let transport_time = self.timers.find(SHOW_TIME).and_then(Stopwatch::elapsed);
            crate::util::stats::process_statistics(
                &connection.stats(),
                aggregate_stats,
                transport_time,
                &remote_stats,
                &config,
                self.args.client_params.statistics,
                direction,
            );
        }

        if self.args.client_params.profile {
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

        let job_specs: Vec<CopyJobSpec> = self.args.jobspecs()?;
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
            &self.args.client_params,
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
                &self.args.client_params,
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

    async fn transfer_jobs<C: BiStreamOpener>(
        &self,
        connection: &C,
        jobs: &[CopyJobSpec],
        config: &Configuration,
        compat: Compatibility,
    ) -> anyhow::Result<(bool, CommandStats)> {
        process_job_requests(
            jobs,
            || connection.open_bi_stream(),
            |stream_pair, job, filename_width| {
                self.manage_request(stream_pair, job, config, compat, filename_width)
            },
            Some(&self.spinner),
        )
        .await
    }

    /// Do whatever it is we were asked to.
    /// On success: returns statistics about the transfer.
    /// On error: returns the transfer statistics, as far as we know, up to the point of failure
    async fn manage_request<S, R>(
        &self,
        stream_pair: SendReceivePair<S, R>,
        copy_spec: CopyJobSpec,
        config: &Configuration,
        compat: Compatibility,
        filename_width: usize,
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
        let filename = copy_spec.display_filename().to_string_lossy();
        let timer = std::time::Instant::now();
        let result = handler
            .send(
                &copy_spec,
                display,
                filename_width,
                spinner,
                config,
                self.args.client_params,
            )
            .instrument(span)
            .await;
        let elapsed = timer.elapsed();

        match result {
            Ok(st) => {
                info!(
                    "{filename}: transferred {}",
                    format_rate(st.payload_bytes, Some(elapsed), st.peak_transfer_rate,)
                );
                RequestResult::new(true, st)
            }
            Err(e) => {
                if let Some(src) = e.source() {
                    // Some error conditions come with an anyhow Context.
                    // We want to output one tidy line, so glue them together.
                    error!("{e}: {src}");
                } else {
                    error!("{e}");
                }
                RequestResult::new(false, CommandStats::default())
            }
        }
    }
}

async fn process_job_requests<S, R, OpenStream, OpenFut, HandleJob, HandleFut>(
    jobs: &[CopyJobSpec],
    mut open_stream: OpenStream,
    mut handle_job: HandleJob,
    spinner: Option<&ProgressBar>,
) -> anyhow::Result<(bool, CommandStats)>
where
    OpenStream: FnMut() -> OpenFut,
    OpenFut: Future<Output = anyhow::Result<SendReceivePair<S, R>>>,
    HandleJob: FnMut(SendReceivePair<S, R>, CopyJobSpec, usize) -> HandleFut,
    HandleFut: Future<Output = RequestResult>,
    S: SendingStream + 'static,
    R: ReceivingStream + 'static,
{
    let mut aggregate_stats = CommandStats::default();
    let mut overall_success = true;
    let filename_width = longest_filename(jobs);
    let n_jobs = jobs.len();

    for (index, job) in jobs.iter().enumerate() {
        if n_jobs > 1
            && let Some(spinner) = spinner
        {
            spinner.set_message(format!(
                "Transferring data (file {} of {n_jobs})",
                index + 1,
            ));
        }
        let stream_pair = open_stream().await?;
        let result = handle_job(stream_pair, job.clone(), filename_width).await;

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

fn longest_filename(jobs: &[CopyJobSpec]) -> usize {
    let mut result = 0;
    for j in jobs {
        result = result.max(j.display_filename().len());
    }
    result
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use indicatif::MultiProgress;
    use std::path::Path;
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    use std::{
        net::{Ipv4Addr, SocketAddrV4},
        str::FromStr,
    };
    use tokio::io::AsyncWriteExt;
    use tokio::time::{Duration, timeout};

    use async_trait::async_trait;
    use littertray::LitterTray;

    use super::{BiStreamOpener, RequestResult, process_job_requests};

    use crate::cli::CliArgs;
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
        let mut args = Box::new(CliArgs {
            paths: vec![
                FileSpec::from_str(src).unwrap(),
                FileSpec::from_str(dest).unwrap(),
            ],
            ..Default::default()
        });

        f(&mut mgr, &mut args.client_params);

        Client::new(Manager::without_default(None), MultiProgress::new(), args).unwrap()
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
        assert!(!res.preserve());
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

    #[cfg_attr(target_os = "macos", ignore)]
    #[cfg_attr(target_os = "windows", ignore = "fails under Wine in CI")]
    #[tokio::test]
    async fn quinn_connection_open_bi_stream_adapter_works() {
        use crate::protocol::control::{Compatibility, ConnectionType};
        use crate::transport::ThroughputMode;
        use crate::util::Credentials;

        let compat = Compatibility::Level(1);
        let config = Configuration::system_default();

        let server_creds = Credentials::generate().unwrap();
        let client_creds = Credentials::generate().unwrap();
        let server_cert = server_creds.to_tagged_data(compat, None).unwrap();
        let client_cert = client_creds.to_tagged_data(compat, None).unwrap();

        let (server_endpoint, _) = crate::control::create_endpoint(
            &server_creds,
            &client_cert,
            ConnectionType::Ipv4,
            config,
            ThroughputMode::Both,
            true,
            compat,
        )
        .unwrap();
        let server_port = server_endpoint.local_addr().unwrap().port();
        let server_addr: std::net::SocketAddr =
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, server_port).into();

        let server_task = tokio::spawn(async move {
            let incoming = timeout(Duration::from_secs(5), server_endpoint.accept())
                .await
                .expect("timed out waiting for QUIC connection")
                .expect("endpoint closed unexpectedly");

            let connection = incoming.await.expect("incoming connection failed");
            let _ = connection.accept_bi().await.expect("accept_bi failed");

            server_endpoint.close(0u32.into(), "test".as_bytes());
            server_endpoint.wait_idle().await;
        });

        let (client_endpoint, _) = crate::control::create_endpoint(
            &client_creds,
            &server_cert,
            ConnectionType::Ipv4,
            config,
            ThroughputMode::Both,
            false,
            compat,
        )
        .unwrap();

        let connecting = client_endpoint
            .connect(server_addr, &server_creds.hostname)
            .unwrap();
        let connection = timeout(Duration::from_secs(5), connecting)
            .await
            .expect("timed out connecting")
            .expect("connection failed");

        let _ = connection.open_bi_stream().await.unwrap();

        connection.close(0u32.into(), "test".as_bytes());
        client_endpoint.close(0u32.into(), "test".as_bytes());
        let _ = timeout(Duration::from_secs(5), client_endpoint.wait_idle()).await;

        let _ = timeout(Duration::from_secs(5), server_task).await;
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
            crate::protocol::control::Compatibility::Level(1),
            10,
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
            crate::protocol::control::Compatibility::Level(1),
            10,
        );
        let r = manage_fut.await;
        println!("Result: {r:?}");
    }

    #[tokio::test]
    async fn transfer_jobs_copies_multiple_files_over_reused_connection() {
        const DATA1: &[u8] = b"alpha";
        const DATA2: &[u8] = b"beta beta";
        const OUT1: &str = "out1";
        const OUT2: &str = "out2";

        let uut = make_uut(|_, _| (), "127.0.0.1:file", OUT1);

        let jobs = vec![
            CopyJobSpec::from_parts("127.0.0.1:file1", OUT1, false, false).unwrap(),
            CopyJobSpec::from_parts("127.0.0.1:file2", OUT2, false, false).unwrap(),
        ];

        let conn = FakeBiConnection::new(vec![
            encode_get_success_response(DATA1),
            encode_get_success_response(DATA2),
        ]);

        let (success, stats) = LitterTray::try_with_async(async |_| {
            let (success, stats) = uut
                .transfer_jobs(
                    &conn,
                    &jobs,
                    Configuration::system_default(),
                    crate::protocol::control::Compatibility::Level(1),
                )
                .await
                .unwrap();

            assert_eq!(std::fs::read(OUT1)?, DATA1);
            assert_eq!(std::fs::read(OUT2)?, DATA2);

            Ok((success, stats))
        })
        .await
        .unwrap();

        assert!(success);
        assert_eq!(conn.open_calls.load(Ordering::SeqCst), 2);
        assert_eq!(stats.payload_bytes, (DATA1.len() + DATA2.len()) as u64);
    }

    #[tokio::test]
    async fn transfer_jobs_stops_after_failure() {
        const DATA1: &[u8] = b"alpha";
        const OUT1: &str = "out1";
        const OUT2: &str = "out2";
        const OUT3: &str = "out3";

        let uut = make_uut(|_, _| (), "127.0.0.1:file", OUT1);

        let jobs = vec![
            CopyJobSpec::from_parts("127.0.0.1:file1", OUT1, false, false).unwrap(),
            CopyJobSpec::from_parts("127.0.0.1:file2", OUT2, false, false).unwrap(),
            CopyJobSpec::from_parts("127.0.0.1:file3", OUT3, false, false).unwrap(),
        ];

        let conn = FakeBiConnection::new(vec![
            encode_get_success_response(DATA1),
            encode_get_error_response(),
        ]);

        let (success, stats) = LitterTray::try_with_async(async |_| {
            let (success, stats) = uut
                .transfer_jobs(
                    &conn,
                    &jobs,
                    Configuration::system_default(),
                    crate::protocol::control::Compatibility::Level(1),
                )
                .await
                .unwrap();

            assert_eq!(std::fs::read(OUT1)?, DATA1);
            assert!(!Path::new(OUT2).exists());
            assert!(!Path::new(OUT3).exists());

            Ok((success, stats))
        })
        .await
        .unwrap();

        assert!(!success);
        assert_eq!(conn.open_calls.load(Ordering::SeqCst), 2);
        assert_eq!(stats.payload_bytes, DATA1.len() as u64);
    }

    #[tokio::test]
    async fn process_job_requests_aggregates_stats() {
        let jobs = vec![
            CopyJobSpec::from_parts("file1", "host:dir", false, false).unwrap(),
            CopyJobSpec::from_parts("file2", "host:dir", false, false).unwrap(),
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
            |stream_pair, _job, _filename_width| {
                let _ = handle_calls.fetch_add(1, Ordering::SeqCst);
                drop(stream_pair);
                async { results.lock().unwrap().remove(0) }
            },
            None,
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
            CopyJobSpec::from_parts("file1", "host:dir", false, false).unwrap(),
            CopyJobSpec::from_parts("file2", "host:dir", false, false).unwrap(),
            CopyJobSpec::from_parts("file3", "host:dir", false, false).unwrap(),
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
            RequestResult::new(false, CommandStats::default()),
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
            |stream_pair, _job, _filename_width| {
                let _ = handle_calls.fetch_add(1, Ordering::SeqCst);
                drop(stream_pair);
                async { results.lock().unwrap().remove(0) }
            },
            None,
        )
        .await
        .unwrap();

        assert!(!success);
        assert_eq!(open_calls.load(Ordering::SeqCst), 2);
        assert_eq!(handle_calls.load(Ordering::SeqCst), 2);
        assert_eq!(stats.payload_bytes, 10);
        assert_eq!(stats.peak_transfer_rate, 100);
    }

    fn encode_get_success_response(data: &[u8]) -> Vec<u8> {
        let mut send_buf = Vec::new();
        crate::protocol::session::Response::V1(crate::protocol::session::ResponseV1 {
            status: crate::protocol::session::Status::Ok.into(),
            message: None,
        })
        .to_writer_framed(&mut send_buf)
        .unwrap();
        crate::protocol::session::FileHeader::new_v1(data.len() as u64, "file")
            .to_writer_framed(&mut send_buf)
            .unwrap();
        send_buf.extend_from_slice(data);
        crate::protocol::session::FileTrailer::V1
            .to_writer_framed(&mut send_buf)
            .unwrap();
        send_buf
    }

    fn encode_get_error_response() -> Vec<u8> {
        let mut send_buf = Vec::new();
        crate::protocol::session::Response::V1(crate::protocol::session::ResponseV1 {
            status: crate::protocol::session::Status::FileNotFound.into(),
            message: Some("nope".to_string()),
        })
        .to_writer_framed(&mut send_buf)
        .unwrap();
        send_buf
    }

    struct FakeBiConnection {
        responses: Mutex<Vec<Vec<u8>>>,
        open_calls: AtomicUsize,
    }

    impl FakeBiConnection {
        fn new(responses: Vec<Vec<u8>>) -> Self {
            Self {
                responses: Mutex::new(responses),
                open_calls: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl BiStreamOpener for FakeBiConnection {
        type Send = tokio::io::WriteHalf<tokio::io::SimplexStream>;
        type Recv = tokio::io::ReadHalf<tokio::io::SimplexStream>;

        async fn open_bi_stream(
            &self,
        ) -> anyhow::Result<crate::protocol::common::SendReceivePair<Self::Send, Self::Recv>>
        {
            let (client_side, mut server_side) = new_test_plumbing();
            let _ = self.open_calls.fetch_add(1, Ordering::SeqCst);
            let response = self.responses.lock().unwrap().remove(0);
            std::mem::drop(tokio::spawn(async move {
                let _ = server_side.send.write_all(&response).await;
            }));
            Ok(client_side)
        }
    }

    #[test]
    fn longest_filenames() {
        use super::longest_filename;
        let jobs = [
            CopyJobSpec::from_parts("server:somedir/file1", "otherdir/file2", false, false)
                .unwrap(),
            CopyJobSpec::from_parts("s:somedir/a", "a", false, false).unwrap(),
            CopyJobSpec::from_parts(
                "s:really/really-long-name",
                "this-name-is-even-longer-but-loses-as-it-is-destination",
                false,
                false,
            )
            .unwrap(),
        ];
        assert_eq!(longest_filename(&jobs), 16);
    }
}
