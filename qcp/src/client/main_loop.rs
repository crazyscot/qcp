//! Main client mode event loop
// (c) 2024 Ross Younger

use crate::{
    FileSpec,
    cli::{CliArgs, styles::use_colours},
    client::progress::SPINNER_TEMPLATE,
    config::{Configuration, Configuration_Optional, Manager},
    control::{ControlChannel, create, create_endpoint},
    protocol::{
        FindTag, TaggedData,
        common::{ReceivingStream, SendReceivePair, SendingStream},
        compat::Feature,
        control::{ClosedownReportV1, Compatibility, CredentialsType, Direction, ServerMessageV2},
        session::{CommandParam, Get2Args, MetadataAttr, Response, Status},
    },
    session::{self, CommandStats, RequestResult},
    util::{
        self, Credentials, lookup_host_by_family,
        path::add_pathsep_if_needed,
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
    net::{IpAddr, SocketAddr, SocketAddrV4, SocketAddrV6},
    path::MAIN_SEPARATOR,
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

struct Client {
    manager: Manager,
    display: MultiProgress,
    credentials: Credentials,
    timers: StopwatchChain,
    spinner: ProgressBar,
    args: Box<CliArgs>,
    /// Before control channel negotiation, this is `None`.
    /// After negotiation, this holds the agreed configuration and may be assumed to be `Some`.
    negotiated: Option<Negotiated>,
}

/// Items negotiated between client and server
struct Negotiated {
    config: Configuration,
    compat: Compatibility,
}

#[derive(Debug, PartialEq)]
struct PrepResult {
    remote_address: IpAddr,
    job_specs: Vec<CopyJobSpec>,
    full_success: bool,
}

enum Phase {
    // ListFiles
    Pre,
    // Get, Put, Mkdir
    Transfer,
    // SetMetadata to fix up directory permissions once we're done
    Post,
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
            negotiated: None,
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
            return Ok(prep_result.full_success);
        }

        // Data channel ------------------

        let connection = self
            .establish_data_channel(&prep_result, &config, &mut qcp_conn)
            .await?;

        // Show time! ---------------------

        let direction = prep_result.direction();
        self.spinner.set_message("Transferring data");
        self.timers.next(SHOW_TIME);
        self.negotiated = Some(Negotiated {
            config,
            compat: qcp_conn.control.selected_compat,
        });
        let (overall_success, aggregate_stats) = self
            .process_job_requests(
                &prep_result.job_specs,
                || connection.open_bi_stream(),
                |stream_pair, job, filename_width, pass| {
                    self.run_request(stream_pair, job, filename_width, pass)
                },
            )
            .await?;

        // Closedown ----------------------
        let remote_stats = self.closedown(qcp_conn).await?;

        // Post-transfer chatter -----------
        if !self.args.client_params.quiet {
            let transport_time = self.timers.find(SHOW_TIME).and_then(Stopwatch::elapsed);
            crate::util::stats::process_statistics(
                &connection.stats(),
                aggregate_stats,
                transport_time,
                &remote_stats,
                &self.negotiated.as_ref().unwrap().config,
                self.args.client_params.statistics,
                direction,
            );
        }

        if self.args.client_params.profile {
            info!("Elapsed time by phase:\n{}", self.timers);
        }
        self.display.clear()?;
        Ok(overall_success & prep_result.full_success)
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

        let (full_success, job_specs) = self.args.jobspecs()?;
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
            full_success,
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
        mut conn: QcpConnection, // ctrl_result is consumed
    ) -> anyhow::Result<ClosedownReportV1> {
        let config = &self.negotiated.as_ref().unwrap().config;
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
    async fn run_request<S, R>(
        &self,
        stream_pair: SendReceivePair<S, R>,
        copy_spec: CopyJobSpec,
        filename_width: usize,
        pass: Phase,
    ) -> Result<RequestResult>
    where
        S: SendingStream + 'static,
        R: ReceivingStream + 'static,
    {
        assert!(
            self.negotiated.is_some(),
            "logic error: run_request called before negotiation completed"
        );
        match pass {
            Phase::Pre => {
                self.manage_pre_transfer_request(stream_pair, &copy_spec)
                    .await
            }
            Phase::Transfer => {
                self.manage_file_transfer_request(stream_pair, &copy_spec, filename_width)
                    .await
            }
            Phase::Post => {
                self.manage_post_transfer_request(stream_pair, &copy_spec)
                    .await
            }
        }
    }

    async fn manage_pre_transfer_request<S, R>(
        &self,
        stream_pair: SendReceivePair<S, R>,
        copy_spec: &CopyJobSpec,
    ) -> Result<RequestResult>
    where
        S: SendingStream + 'static,
        R: ReceivingStream + 'static,
    {
        let negotiated = self.negotiated.as_ref().unwrap(); // checked in run_request
        assert!(
            copy_spec.source.user_at_host.is_some(),
            "logic error: manage_pre_transfer_request called for local source"
        );

        let mut cmd = session::Listing::boxed(stream_pair, None, negotiated.compat);
        cmd.send(
            copy_spec,
            self.display.clone(),
            0,
            self.spinner.clone(),
            &negotiated.config,
            self.args.client_params,
        )
        .await
    }

    async fn manage_file_transfer_request<S, R>(
        &self,
        stream_pair: SendReceivePair<S, R>,
        copy_spec: &CopyJobSpec,
        filename_width: usize,
    ) -> Result<RequestResult>
    where
        S: SendingStream + 'static,
        R: ReceivingStream + 'static,
    {
        use crate::session;

        let negotiated = self.negotiated.as_ref().unwrap(); // checked in run_request

        let (mut cmd, span) = if copy_spec.source.user_at_host.is_some() {
            // We are GETting something
            let mut args = Get2Args::default();
            if copy_spec.preserve {
                args.options.push(CommandParam::PreserveMetadata.into());
            }
            (
                session::Get::boxed(stream_pair, Some(args), negotiated.compat),
                trace_span!("GETx", filename = copy_spec.source.filename.clone()),
            )
        } else {
            // We are PUTting something
            if copy_spec.directory {
                (
                    session::CreateDirectory::boxed(stream_pair, None, negotiated.compat),
                    trace_span!("MKDIR", filename = copy_spec.destination.filename.clone()),
                )
            } else {
                (
                    session::Put::boxed(stream_pair, None, negotiated.compat),
                    trace_span!("PUTx", filename = copy_spec.source.filename.clone()),
                )
            }
        };
        let filename = copy_spec.display_filename().to_string_lossy();
        let timer = std::time::Instant::now();
        let result = cmd
            .send(
                copy_spec,
                self.display.clone(),
                filename_width,
                self.spinner.clone(),
                &negotiated.config,
                self.args.client_params,
            )
            .instrument(span)
            .await;
        let elapsed = timer.elapsed();
        result.inspect(|rr| {
            info!(
                "{filename}: transferred {}",
                format_rate(
                    rr.stats.payload_bytes,
                    Some(elapsed),
                    rr.stats.peak_transfer_rate,
                )
            );
        })
    }

    async fn manage_post_transfer_request<S, R>(
        &self,
        stream_pair: SendReceivePair<S, R>,
        copy_spec: &CopyJobSpec,
    ) -> Result<RequestResult>
    where
        S: SendingStream + 'static,
        R: ReceivingStream + 'static,
    {
        let negotiated = self.negotiated.as_ref().unwrap(); // checked in run_request
        let destination_is_remote = copy_spec.destination.user_at_host.is_some();

        if destination_is_remote && copy_spec.preserve && copy_spec.directory {
            return session::SetMetadata::boxed(stream_pair, None, negotiated.compat)
                .send(
                    copy_spec,
                    self.display.clone(),
                    0,
                    self.spinner.clone(),
                    &negotiated.config,
                    self.args.client_params,
                )
                .await;
        }
        if !destination_is_remote && let Some(mode) = copy_spec.mode {
            let perms = tokio::fs::metadata(&copy_spec.destination.filename)
                .await
                .map(|m| m.permissions())
                .map(|mut perms| {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt as _;
                        perms.set_mode(mode);
                    }
                    #[cfg(windows)]
                    // As for files, map _any_ writeable bit into writeability.
                    perms.set_readonly((mode & 0o222) == 0);
                    perms
                });
            match perms {
                Ok(p) => tokio::fs::set_permissions(&copy_spec.destination.filename, p).await,
                Err(e) => Err(e),
            }?;
        }
        // else do nothing
        Ok(RequestResult::new(CommandStats::default(), None))
    }

    async fn process_job_requests<S, R, OpenStream, JobRunner>(
        &self,
        jobs_in: &[CopyJobSpec],
        mut open_stream: OpenStream,
        mut run_job: JobRunner,
    ) -> anyhow::Result<(bool, CommandStats)>
    where
        OpenStream: AsyncFnMut() -> anyhow::Result<SendReceivePair<S, R>>,
        JobRunner:
            AsyncFnMut(SendReceivePair<S, R>, CopyJobSpec, usize, Phase) -> Result<RequestResult>,
        S: SendingStream + 'static,
        R: ReceivingStream + 'static,
    {
        let destination_is_remote = jobs_in
            .first()
            .is_some_and(|j| j.destination.user_at_host.is_some());
        let recurse: bool = self.args.client_params.recurse;

        if !destination_is_remote && recurse {
            self.process_recursive_get(jobs_in, async move || open_stream().await, &mut run_job)
                .await
        } else {
            self.process_file_transfers(jobs_in, async move || open_stream().await, &mut run_job)
                .await
        }
    }

    /// This function should generally log errors and return Ok(status, stats). Err(...) is reserved for fatal errors.
    async fn process_file_transfers<S, R, OpenStream, JobRunner>(
        &self,
        jobs: &[CopyJobSpec],
        mut open_stream: OpenStream,
        mut run_job: JobRunner,
    ) -> anyhow::Result<(bool, CommandStats)>
    where
        OpenStream: AsyncFnMut() -> anyhow::Result<SendReceivePair<S, R>>,
        JobRunner:
            AsyncFnMut(SendReceivePair<S, R>, CopyJobSpec, usize, Phase) -> Result<RequestResult>,
        S: SendingStream + 'static,
        R: ReceivingStream + 'static,
    {
        let mut aggregate_stats = CommandStats::default();
        let mut overall_success = true;

        let destination_is_remote = jobs
            .first()
            .is_some_and(|j| j.destination.user_at_host.is_some());

        // FILE TRANSFER PHASE
        // Send/receive files and create directories.
        // The list of job specs must be in the appropriate order i.e. create a directory before attempting to put any files into it.

        let filename_width = longest_filename(jobs);
        let n_jobs = jobs.len();
        let n_files = jobs.iter().filter(|j| !j.directory).count();
        let mut files_done = 0;
        for job in jobs {
            if n_files > 1 {
                self.spinner.set_message(format!(
                    "Transferring data (file {} of {n_files})",
                    files_done + 1,
                ));
            }
            if !destination_is_remote && job.directory {
                // Local directory creation is trivial
                debug!("Creating local directory {}", job.destination.filename);
                let meta = tokio::fs::metadata(&job.destination.filename).await;
                if let Ok(m) = meta {
                    if m.is_file() {
                        error!(
                            "Cannot create local directory {}: a file already exists there",
                            job.destination.filename
                        );
                        overall_success = false;
                        break;
                    }
                    // directory already exists, that's fine
                    continue;
                }
                if let Err(e) = tokio::fs::create_dir_all(&job.destination.filename).await
                    && e.kind() != std::io::ErrorKind::AlreadyExists
                {
                    error!(
                        "Failed to create local directory {}: {e}",
                        job.destination.filename
                    );
                    overall_success = false;
                    break;
                }
                continue;
            }
            debug!("Processing job {:?}", job);
            let stream_pair = open_stream().await?;
            let result = run_job(stream_pair, job.clone(), filename_width, Phase::Transfer).await;
            match result {
                Ok(result) => {
                    aggregate_stats.payload_bytes += result.stats.payload_bytes;
                    aggregate_stats.peak_transfer_rate = aggregate_stats
                        .peak_transfer_rate
                        .max(result.stats.peak_transfer_rate);
                }
                Err(e) => {
                    if let Some(src) = e.source() {
                        // Some error conditions come with an anyhow Context.
                        // We want to output one tidy line, so glue them together.
                        error!("{e}: {src}");
                    } else {
                        error!("{e}");
                    }
                    overall_success = false;
                    break;
                }
            }
            files_done += 1;
        }

        // POST-TRANSFER: Apply preserve logic (permission bits) to any directories created.
        // We do this in _reverse order_ in case the changed permissions prevent us from being able to traverse a directory we recently created.
        if n_jobs > 1 {
            let mut message_set = false;
            for job in jobs.iter().rev() {
                if job.directory && job.preserve {
                    let stream_pair = open_stream().await?;
                    if !message_set {
                        self.spinner
                            .set_message("Finishing up directory permissions");
                        message_set = true;
                    }
                    let result = run_job(stream_pair, job.clone(), 0, Phase::Post).await;
                    if let Err(e) = result {
                        if let Some(src) = e.source() {
                            // Some error conditions come with an anyhow Context.
                            // We want to output one tidy line, so glue them together.
                            error!("{e}: {src}");
                        } else {
                            error!("{e}");
                        }
                        overall_success = false;
                    }
                }
            }
        }

        Ok((overall_success, aggregate_stats))
    }

    /// This function should generally log errors and return Ok(status, stats). Err(...) is reserved for fatal errors.
    #[allow(clippy::too_many_lines)]
    async fn process_recursive_get<S, R, OpenStream, JobRunner>(
        &self,
        jobs_in: &[CopyJobSpec],
        mut open_stream: OpenStream,
        mut run_job: JobRunner,
    ) -> anyhow::Result<(bool, CommandStats)>
    where
        OpenStream: AsyncFnMut() -> anyhow::Result<SendReceivePair<S, R>>,
        JobRunner:
            AsyncFnMut(SendReceivePair<S, R>, CopyJobSpec, usize, Phase) -> Result<RequestResult>,
        S: SendingStream + 'static,
        R: ReceivingStream + 'static,
    {
        anyhow::ensure!(
            self.negotiated
                .as_ref()
                .map(|n| n.compat)
                .unwrap_or_default()
                .supports(Feature::MKDIR_SETMETA_LS),
            "Operation not supported by remote"
        );

        // If this is a recursive GET, check the local destination directory to fail fast.
        // We try very hard to match scp behaviour here:
        // - with one source:
        //   - if the destination does not exist, and the source is a directory, create it; put the *contents* of remote folders into it (!)
        //   - if the destination does not exist, and the source is a file, create it; put the *contents* of remote folders into it (!)
        //   - if the destination exists and is a file, error; if it is a directory, copy the remote folders into it
        // - with multiple sources:
        //   - if the root (user entered) destination directory does not exist, error
        //   - if the destination exists and is a file, error; if a directory, copy remote folders into it
        //
        // We signify the special single-source mode (marked '!' above) by setting single_source_mkdir_mode to Some(output_dir_name).
        // We don't actually create the directory until we're sure we need to (i.e. we know from the remote that it is indeed a directory).

        let original_dest_dir = &jobs_in
            .first()
            .expect("logic error: empty jobs list in recursive GET")
            .destination
            .filename;
        let dest_meta = tokio::fs::metadata(original_dest_dir).await;
        let single_source_mkdir_mode = if let Ok(meta) = dest_meta {
            anyhow::ensure!(
                !meta.is_file(),
                "Local destination directory is a file: {original_dest_dir}"
            );
            // Directory exists; no special action needed.
            None
        } else {
            // Directory does not exist
            anyhow::ensure!(
                jobs_in.len() == 1,
                "Local destination directory {original_dest_dir} does not exist; with multiple source files/directories, the destination directory must exist",
            );
            // This is the special-case mode. The local destination directory doesn't exist; we will create it in a moment.
            // (Not too soon, in case we bail before getting to the actual file transfer.)
            Some(original_dest_dir.clone())
        };

        // PRE-TRANSFER:
        // If this is a recursive GET, ask the remote to enumerate the files.
        let mut success = true;
        self.spinner
            .set_message("Asking remote for list of files to transfer");
        let mut new_jobs = Vec::new();
        for job in jobs_in {
            let stream_pair = open_stream().await?;
            let result = run_job(stream_pair, job.clone(), 0, Phase::Pre).await?;
            let Some(Response::List(contents)) = result.response else {
                anyhow::bail!(
                    "logic error: pre-transfer request did not return List response data"
                );
            };
            if !Status::try_from(contents.status).is_ok_and(|s| s == Status::Ok) {
                let with_message = if contents.message.is_some() {
                    " with message "
                } else {
                    ""
                };
                error!(
                    "Failed to list contents of '{}': server returned status {st}{with_message}{message}",
                    job.source,
                    st = Status::to_string(contents.status),
                    message = contents.message.unwrap_or_default(),
                );
                success = false;
            }
            for item in contents.entries {
                let mut destfile = job.destination.filename.clone();
                let leaf = item
                    .name
                    .strip_prefix(&job.source.filename)
                    .unwrap_or(&item.name)
                    .trim_start_matches(MAIN_SEPARATOR);
                trace!("dest {destfile}");
                if single_source_mkdir_mode.is_none() {
                    // In normal mode, we need to add the remote directory name as well.
                    let remote_dir_name = std::path::Path::new(&job.source.filename)
                        .file_name()
                        .and_then(|os_str| os_str.to_str())
                        .unwrap_or_default();
                    if !remote_dir_name.is_empty() {
                        add_pathsep_if_needed(&mut destfile, true);
                        destfile.push_str(remote_dir_name);
                        trace! {"1smkdir: {destfile}"};
                    }
                }
                if !leaf.is_empty() {
                    add_pathsep_if_needed(&mut destfile, true);
                    destfile.push_str(leaf);
                }
                trace!(
                    "source path {name}; leaf {leaf:?}; final dest {destfile}",
                    name = item.name
                );
                #[allow(clippy::cast_possible_truncation)]
                new_jobs.push(CopyJobSpec {
                    user_at_host: job.user_at_host.clone(),
                    source: FileSpec {
                        user_at_host: job.source.user_at_host.clone(),
                        filename: item.name,
                    },
                    destination: FileSpec {
                        user_at_host: job.destination.user_at_host.clone(),
                        filename: destfile,
                    },
                    directory: item.directory,
                    preserve: job.preserve,
                    mode: item
                        .attributes
                        .find_tag(MetadataAttr::ModeBits)
                        .map(|i| i.coerce_unsigned() as u32),
                });
            }
        }

        if !success {
            warn!("No files were transferred");
            return Ok((false, CommandStats::default()));
        }

        // Now, if required, handle single-source mkdir mode.
        if let Some(dir_to_create) = single_source_mkdir_mode
            && !new_jobs.is_empty()
        {
            if new_jobs[0].directory {
                debug!("single source mode; item is a directory; creating it");
                tokio::fs::create_dir_all(&dir_to_create)
                    .await
                    .context(format!(
                        "while creating local destination directory {dir_to_create}",
                    ))?;
            } else {
                debug!("single source mode; item is a file");
            }
        }

        let new_jobs = new_jobs;

        self.process_file_transfers(&new_jobs, async move || open_stream().await, &mut run_job)
            .await
    }
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

    use super::{BiStreamOpener, RequestResult};

    use crate::cli::CliArgs;
    use crate::client::main_loop::{Negotiated, Phase};
    #[cfg(unix)]
    use crate::control::create_fake;

    use crate::session::CommandStats;
    use crate::{
        Configuration, CopyJobSpec, FileSpec, Parameters,
        client::main_loop::Client,
        config::{Configuration_Optional, Manager},
        protocol::{common::ProtocolMessage as _, test_helpers::new_test_plumbing},
    };

    mod get_multi;

    fn make_uut<F: FnOnce(&mut Manager, &mut Parameters)>(
        f: F,
        src: &str,
        dest: &str,
        compat_level: u16,
    ) -> Client {
        make_uut_multi(f, &[src], dest, compat_level)
    }

    fn make_uut_multi<F: FnOnce(&mut Manager, &mut Parameters)>(
        f: F,
        src: &[&str],
        dest: &str,
        compat_level: u16,
    ) -> Client {
        let mut mgr = Manager::without_default(None);
        let mut paths = src
            .iter()
            .map(|s| FileSpec::from_str(s).unwrap())
            .collect::<Vec<_>>();
        paths.push(FileSpec::from_str(dest).unwrap());
        let mut args = Box::new(CliArgs {
            paths,
            ..Default::default()
        });

        f(&mut mgr, &mut args.client_params);

        let mut client =
            Client::new(Manager::without_default(None), MultiProgress::new(), args).unwrap();
        client.negotiated = Some(Negotiated {
            config: Configuration::system_default().clone(),
            compat: crate::protocol::control::Compatibility::Level(compat_level),
        });
        client
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
        let mut uut = make_uut(|_, _| (), REMOTE_FILE, LOCAL_FILE, 1);
        let working = Configuration_Optional::default();
        let res = uut.prep(&working, Configuration::system_default()).unwrap();
        assert_eq!(res.remote_address, Ipv4Addr::new(8, 8, 8, 8));
        assert_eq!(res.job_specs[0].source, remote_file_spec());
        assert_eq!(res.job_specs[0].destination, local_file_spec());
        assert!(!res.preserve());
        assert!(res.full_success);
        eprintln!("{res:?}");
    }
    #[test]
    fn prep_invalid_hostname() {
        let mut uut = make_uut(|_, _| (), "no-such-host.invalid:file", "file", 1);
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

        let mut uut = make_uut(|_, _| (), "127.0.0.1:file", LOCAL_FILE, 1);
        let working = Configuration_Optional::default();
        let config = Configuration::system_default().clone();
        let prep_result = uut.prep(&working, Configuration::system_default()).unwrap();
        let server_cert = crate::util::Credentials::generate().unwrap();
        let server_address_port = (Ipv4Addr::LOCALHOST, 0);
        let level = Compatibility::Level(1);
        assert!(prep_result.full_success);

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

        let report = uut.closedown(qcp_conn).await.unwrap();
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
        let mut uut = make_uut(|_, _| (), "127.0.0.1:file", "outfile", 1);
        let working = Configuration_Optional::default();
        let prep_result = uut.prep(&working, Configuration::system_default()).unwrap();
        assert!(prep_result.full_success);
        let mut plumbing = new_test_plumbing();

        let manage_fut = uut.run_request(
            plumbing.0,
            prep_result.job_specs[0].clone(),
            10,
            Phase::Transfer,
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
        .unwrap()
        .unwrap();
        println!("Result: {r:?}");
        assert_eq!(r.stats.payload_bytes, TEST_DATA.len() as u64);
    }

    #[tokio::test]
    async fn handle_put_failing() {
        let mut uut = make_uut(|_, _| (), "/tmp/file", "127.0.0.1:file", 1);
        let working = Configuration_Optional::default();
        let prep_result = uut.prep(&working, Configuration::system_default()).unwrap();
        assert!(prep_result.full_success);
        let mut plumbing = new_test_plumbing();
        plumbing.1.send.shutdown().await.unwrap(); // this causes the handler to error out

        let manage_fut = uut.run_request(
            plumbing.0,
            prep_result.job_specs[0].clone(),
            10,
            Phase::Transfer,
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

        let uut = make_uut(|_, _| (), "127.0.0.1:file", OUT1, 1);

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
                .process_job_requests(
                    &jobs,
                    || conn.open_bi_stream(),
                    |stream_pair, job, filename_width, pass| {
                        uut.run_request(stream_pair, job, filename_width, pass)
                    },
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

        let uut = make_uut(|_, _| (), "127.0.0.1:file", OUT1, 1);

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
                .process_job_requests(
                    &jobs,
                    || conn.open_bi_stream(),
                    |stream_pair, job, filename_width, pass| {
                        uut.run_request(stream_pair, job, filename_width, pass)
                    },
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
                CommandStats {
                    payload_bytes: 10,
                    peak_transfer_rate: 100,
                },
                None,
            ),
            RequestResult::new(
                CommandStats {
                    payload_bytes: 5,
                    peak_transfer_rate: 200,
                },
                None,
            ),
        ]);

        let client = make_uut(|_, _| (), "src", "dest", 1);
        let (success, stats) = client
            .process_job_requests(
                &jobs,
                || {
                    let _ = open_calls.fetch_add(1, Ordering::SeqCst);
                    async { Ok::<_, anyhow::Error>(new_test_plumbing().0) }
                },
                |stream_pair, _job, _filename_width, _pass| {
                    let _ = handle_calls.fetch_add(1, Ordering::SeqCst);
                    drop(stream_pair);
                    async { Ok(results.lock().unwrap().remove(0)) }
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
            CopyJobSpec::from_parts("file1", "host:dir", false, false).unwrap(),
            CopyJobSpec::from_parts("file2", "host:dir", false, false).unwrap(),
            CopyJobSpec::from_parts("file3", "host:dir", false, false).unwrap(),
        ];

        let open_calls = AtomicUsize::new(0);
        let handle_calls = AtomicUsize::new(0);
        let results = Mutex::new(vec![
            Ok(RequestResult::new(
                CommandStats {
                    payload_bytes: 10,
                    peak_transfer_rate: 100,
                },
                None,
            )),
            Err(anyhow::anyhow!("this one failed")),
            // This would only be consumed if we failed to stop early.
            Ok(RequestResult::new(
                CommandStats {
                    payload_bytes: 999,
                    peak_transfer_rate: 999,
                },
                None,
            )),
        ]);

        let client = make_uut(|_, _| (), "src", "dest", 1);
        let (success, stats) = client
            .process_job_requests(
                &jobs,
                || {
                    let _ = open_calls.fetch_add(1, Ordering::SeqCst);
                    async { Ok::<_, anyhow::Error>(new_test_plumbing().0) }
                },
                |stream_pair, _job, _filename_width, _pass| {
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

    #[tokio::test]
    async fn process_job_requests_handles_directory_preserve() {
        let jobs = vec![
            CopyJobSpec::from_parts("dir1", "host:dir1", true, true).unwrap(),
            CopyJobSpec::from_parts("file", "host:dir1/", true, false).unwrap(),
            CopyJobSpec::from_parts("dir2", "host:dir2", true, true).unwrap(),
        ];

        let open_calls = AtomicUsize::new(0);
        let handle_calls = AtomicUsize::new(0);
        let results = Mutex::new(vec![
            // Each directory is handled twice, so we expect to see five results.
            // pass 1:
            RequestResult::new(CommandStats::default(), None),
            RequestResult::new(
                // this is file1
                CommandStats {
                    payload_bytes: 10,
                    peak_transfer_rate: 100,
                },
                None,
            ),
            RequestResult::new(CommandStats::default(), None),
            // pass 2:
            RequestResult::new(CommandStats::default(), None),
            RequestResult::new(CommandStats::default(), None),
        ]);

        let client = make_uut(|_, _| (), "src", "dest", 1);
        let (success, stats) = client
            .process_job_requests(
                &jobs,
                || {
                    let _ = open_calls.fetch_add(1, Ordering::SeqCst);
                    async { Ok::<_, anyhow::Error>(new_test_plumbing().0) }
                },
                |stream_pair, _job, _filename_width, _pass| {
                    let _ = handle_calls.fetch_add(1, Ordering::SeqCst);
                    drop(stream_pair);
                    async { Ok(results.lock().unwrap().remove(0)) }
                },
            )
            .await
            .unwrap();

        assert!(success);
        assert_eq!(open_calls.load(Ordering::SeqCst), 5);
        assert_eq!(handle_calls.load(Ordering::SeqCst), 5);
        assert_eq!(stats.payload_bytes, 10);
        assert_eq!(stats.peak_transfer_rate, 100);
    }

    #[tokio::test]
    async fn handle_post_transfer() {
        use littertray::LitterTray;

        let mut uut = make_uut(|_, _| (), "srcdir", "127.0.0.1:destdir", 4);
        uut.args.client_params.preserve = true;
        uut.args.client_params.recurse = true;
        let working = Configuration_Optional::default();
        let r = LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("srcdir");
            let _ = tray.make_dir("destdir");
            let prep_result = uut.prep(&working, Configuration::system_default()).unwrap();
            assert!(prep_result.full_success);
            let mut plumbing = new_test_plumbing();

            let manage_fut =
                uut.run_request(plumbing.0, prep_result.job_specs[0].clone(), 0, Phase::Post);

            // We are not really testing the protocol here, only the main loop behaviour.
            let mut send_buf = Vec::new();
            crate::protocol::session::Response::V1(crate::protocol::session::ResponseV1 {
                status: crate::protocol::session::Status::Ok.into(),
                message: None,
            })
            .to_writer_framed(&mut send_buf)
            .unwrap();
            let send_fut = plumbing.1.send.write_all(&send_buf);

            let (a, b) = tokio::join!(send_fut, manage_fut);
            a.unwrap();
            Ok(b)
        })
        .await
        .unwrap()
        .unwrap();
        println!("Result: {r:?}");
        assert_eq!(r.stats.payload_bytes, 0);
    }
}
