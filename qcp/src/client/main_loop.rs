//! Main client mode event loop
// (c) 2024 Ross Younger

use crate::{
    client::{control::Channel, progress::spinner_style},
    config::{Configuration, Configuration_Optional, Manager},
    protocol::{
        common::{ProtocolMessage, StreamPair},
        session::{Command, FileHeader, FileTrailer, GetArgs, PutArgs, Response, Status},
    },
    transport::ThroughputMode,
    util::{
        self, Credentials, TimeFormat, lookup_host_by_family,
        time::{Stopwatch, StopwatchChain},
    },
};

use anyhow::{Context, Result};
use futures_util::TryFutureExt as _;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish};
use quinn::crypto::rustls::QuicClientConfig;
use quinn::{Connection, EndpointConfig, rustls};
use rustls::RootCertStore;
use rustls_pki_types::CertificateDer;
use std::net::{SocketAddr, SocketAddrV4, SocketAddrV6};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::time::Instant;
use tokio::{self, io::AsyncReadExt, time::Duration, time::timeout};
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
        // This async block returns a Result<u64>
        let sp = connection.open_bi().map_err(|e| anyhow::anyhow!(e)).await?;
        // Called function returns its payload size.
        // This async block reports on errors.
        if copy_spec.source.user_at_host.is_some() {
            // This is a Get
            do_get(sp.into(), &copy_spec, display, spinner, &config, quiet)
                .instrument(trace_span!("GET", filename = copy_spec.source.filename))
                .await
        } else {
            // This is a Put
            do_put(sp.into(), &copy_spec, display, spinner, &config, quiet)
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

/// Adds a progress bar to the stack (in `MultiProgress`) for the current job
fn progress_bar_for(
    display: &MultiProgress,
    job: &CopyJobSpec,
    steps: u64,
    quiet: bool,
) -> Result<ProgressBar> {
    if quiet {
        return Ok(ProgressBar::hidden());
    }
    let display_filename = {
        let component = PathBuf::from(&job.source.filename);
        component
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };
    Ok(display.add(
        ProgressBar::new(steps)
            .with_style(indicatif::ProgressStyle::with_template(
                super::progress::progress_style_for(
                    &console::Term::stderr(),
                    display_filename.len(),
                ),
            )?)
            .with_message(display_filename)
            .with_finish(ProgressFinish::Abandon),
    ))
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

/// Actions a GET command
async fn do_get(
    mut stream: StreamPair,
    job: &CopyJobSpec,
    display: MultiProgress,
    spinner: ProgressBar,
    config: &Configuration,
    quiet: bool,
) -> Result<u64> {
    let filename = &job.source.filename;
    let dest = &job.destination.filename;

    let real_start = Instant::now();
    trace!("send command");
    Command::Get(GetArgs {
        filename: filename.to_string(),
    })
    .to_writer_async_framed(&mut stream.send)
    .await?;
    stream.send.flush().await?;

    // TODO protocol timeout?
    trace!("await response");
    let response = Response::from_reader_async_framed(&mut stream.recv).await?;
    let Response::V1(response) = response;
    if response.status != Status::Ok {
        anyhow::bail!(format!("GET ({filename}) failed: {response}"));
    }

    let header = FileHeader::from_reader_async_framed(&mut stream.recv).await?;
    trace!("{header:?}");
    let FileHeader::V1(header) = header;

    let mut file = crate::util::io::create_truncate_file(dest, &header).await?;

    // Now we know how much we're receiving, update the chrome.
    // File Trailers are currently 16 bytes on the wire.

    // Unfortunately, the file data is already well in flight at this point, leading to a flood of packets
    // that causes the estimated rate to spike unhelpfully at the beginning of the transfer.
    // Therefore we incorporate time in flight so far to get the estimate closer to reality.
    let progress_bar = progress_bar_for(&display, job, header.size.0 + 16, quiet)?
        .with_elapsed(Instant::now().duration_since(real_start));

    let mut meter =
        crate::client::meter::InstaMeterRunner::new(&progress_bar, spinner, config.rx());
    meter.start().await;

    let inbound = progress_bar.wrap_async_read(stream.recv);

    let mut inbound = inbound.take(header.size.0);
    trace!("payload");
    let _ = tokio::io::copy(&mut inbound, &mut file).await?;
    // Retrieve the stream from within the Take wrapper for further operations
    let mut inbound = inbound.into_inner();

    trace!("trailer");
    let _trailer = FileTrailer::from_reader_async_framed(&mut inbound).await?;
    // Trailer is empty for now, but its existence means the server believes the file was sent correctly

    // Note that the Quinn send stream automatically calls finish on drop.
    meter.stop().await;
    file.flush().await?;
    trace!("complete");
    progress_bar.finish_and_clear();
    Ok(header.size.0)
}

/// Actions a PUT command
async fn do_put(
    mut stream: StreamPair,
    job: &CopyJobSpec,
    display: MultiProgress,
    spinner: ProgressBar,
    config: &Configuration,
    quiet: bool,
) -> Result<u64> {
    let src_filename = &job.source.filename;
    let dest_filename = &job.destination.filename;

    let path = PathBuf::from(src_filename);
    let (mut file, meta) = match crate::util::io::open_file(src_filename).await {
        Ok(res) => res,
        Err((_, _, error)) => {
            return Err(error.into());
        }
    };
    if meta.is_dir() {
        anyhow::bail!("PUT: Source is a directory");
    }

    let payload_len = meta.len();

    // Now we can compute how much we're going to send, update the chrome.
    // Marshalled commands are currently 48 bytes + filename length
    // File headers are currently 36 + filename length; Trailers are 16 bytes.
    let steps = payload_len + 48 + 36 + 16 + 2 * dest_filename.len() as u64;
    let progress_bar = progress_bar_for(&display, job, steps, quiet)?;
    let mut outbound = progress_bar.wrap_async_write(stream.send);
    let mut meter =
        crate::client::meter::InstaMeterRunner::new(&progress_bar, spinner, config.tx());
    meter.start().await;

    trace!("sending command");

    Command::Put(PutArgs {
        filename: dest_filename.to_string(),
    })
    .to_writer_async_framed(&mut outbound)
    .await?;
    outbound.flush().await?;

    // TODO protocol timeout?
    trace!("await response");
    let response = Response::from_reader_async_framed(&mut stream.recv).await?;
    let Response::V1(response) = response;
    if response.status != Status::Ok {
        anyhow::bail!(format!("PUT ({src_filename}) failed: {response}"));
    }

    // The filename in the protocol is the file part only of src_filename
    trace!("send header");
    let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks
    FileHeader::new_v1(payload_len, protocol_filename)
        .to_writer_async_framed(&mut outbound)
        .await?;

    // A server-side abort might happen part-way through a large transfer.
    trace!("send payload");
    let result = tokio::io::copy(&mut file, &mut outbound).await;

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
                let response = match Response::from_reader_async_framed(&mut stream.recv).await {
                    Err(_) => anyhow::bail!("connection closed unexpectedly"),
                    Ok(r) => r,
                };
                let Response::V1(response) = response;
                anyhow::bail!(
                    "remote closed connection: {:?}: {}",
                    response.status,
                    response.message.unwrap_or("(no message)".into())
                );
            }
            anyhow::bail!(
                "Unknown I/O error during PUT: {e}/{:?}/{:?}",
                e.kind(),
                e.raw_os_error()
            );
        }
    }

    trace!("send trailer");
    FileTrailer::V1
        .to_writer_async_framed(&mut outbound)
        .await?;
    outbound.flush().await?;
    meter.stop().await;

    let response = Response::from_reader_async_framed(&mut stream.recv).await?;
    let Response::V1(response) = response;
    if response.status != Status::Ok {
        anyhow::bail!(format!(
            "PUT ({src_filename}) failed on completion check: {response}"
        ));
    }

    // Note that the Quinn sendstream calls finish() on drop.
    trace!("complete");
    progress_bar.finish_and_clear();
    Ok(payload_len)
}
