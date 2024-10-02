// QCP command-line arguments
// (c) 2024 Ross Younger

use std::process::ExitCode;
use std::str::FromStr;

use crate::{
    build_info, client,
    os::os,
    transport,
    util::{setup_tracing, AddressFamily},
};
use clap::Parser;
use human_units::Size;
use indicatif::MultiProgress;

/// Options that switch us into another mode i.e. which don't require source/destination arguments
const MODE_OPTIONS: &[&str] = &["server", "help_buffers"];

#[derive(Debug, Parser, Clone)]
#[command(
    author,
    version(build_info::GIT_VERSION),
    about,
    before_help = "Example:   qcp some/file my-server:some-directory/",
    infer_long_args(true)
)]
#[command(help_template(
    "\
{name} version {version}
(c) {author-with-newline}{about-with-newline}
{usage-heading} {usage}
{before-help}
{all-args}{after-help}
"
))]
#[command(styles=crate::styles::get())]
pub(crate) struct Cli {
    // MODE SELECTION ======================================================================
    /// Operates in server mode. This is what we run on the remote machine; it is not
    /// intended for interactive use.
    #[arg(
        long, help_heading("Modes"), hide = true,
        conflicts_with_all(["help_buffers", "quiet", "statistics", "timeout", "ipv4", "ipv6", "remote_debug", "profile"])
    )]
    server: bool,

    /// Outputs additional information about kernel UDP buffer sizes and platform-specific tips
    #[arg(long, action, help_heading("Network tuning"))]
    pub help_buffers: bool,

    // CLIENT-ONLY OPTIONS =================================================================
    /// Quiet mode (no statistics or progress, report only errors)
    #[arg(short, long, action)]
    pub quiet: bool,

    /// Outputs additional transfer statistics
    #[arg(short = 's', long, alias("stats"), action, conflicts_with("quiet"))]
    pub statistics: bool,

    /// The connection timeout on the control channel
    #[arg(short, long, default_value("10"), value_name("seconds"))]
    pub timeout: u16,

    /// Forces IPv4 connection (default: autodetect)
    #[arg(short = '4', long, action)]
    pub ipv4: bool,
    /// Forces IPv6 connection (default: autodetect)
    #[arg(short = '6', long, action, conflicts_with("ipv4"))]
    pub ipv6: bool,

    // CLIENT DEBUG ----------------------------
    /// Enable detailed debug output
    #[arg(short, long, action, help_heading("Debug options"), display_order(100))]
    pub debug: bool,
    /// Enables detailed server (remote) debug output
    #[arg(long, action, help_heading("Debug options"))]
    pub remote_debug: bool,
    /// Prints timing profile data after completion
    #[arg(long, action, help_heading("Debug options"))]
    pub profile: bool,
    /// Log to a file. By default the log receives everything printed to stderr.
    /// This can be overridden by setting the environment variable RUST_LOG_FILE_DETAIL (same semantics as RUST_LOG).
    #[arg(short('l'), long, action, help_heading("Debug options"))]
    pub log_file: Option<String>,

    // TUNING OPTIONS ======================================================================
    /// The maximum network bandwidth we expect to/from the target system.
    /// Along with the initial RTT, this directly affects the buffer sizes used.
    /// This may be specified directly as a number of bytes, or as an SI quantity
    /// e.g. "10M" or "256k". Note that this is described in bytes, not bits;
    /// if (for example) you expect to fill a 1Gbit ethernet connection,
    /// 125M might be a suitable upper limit.
    #[arg(short('b'), long, help_heading("Network tuning"), display_order(50), default_value("12M"), value_name="bytes", value_parser=clap::value_parser!(Size))]
    pub bandwidth: Size,

    /// The expected network Round Trip time to the target system, in milliseconds.
    /// Along with the bandwidth limit, this directly affects the buffer sizes used.
    /// (Buffer size = bandwidth * RTT)
    #[arg(
        short('r'),
        long,
        help_heading("Network tuning"),
        default_value("300"),
        value_name("ms")
    )]
    pub rtt: u16,

    /// (Network wizards only! Setting this too high will cause a reduction in throughput.)
    /// The initial value for the sending congestion control window.
    /// qcp uses the CUBIC congestion control algorithm. The window grows by the number of bytes acknowledged each time,
    /// until encountering saturation or congestion.
    #[arg(
        short('w'),
        long,
        help_heading("Network tuning"),
        default_value("14720"),
        value_name = "bytes"
    )]
    pub initial_congestion_window: u64,

    // POSITIONAL ARGUMENTS ================================================================
    /// The source file. This may be a local filename, or remote specified as HOST:FILE.
    #[arg(
        conflicts_with_all(MODE_OPTIONS),
        required = true,
        value_name = "SOURCE"
    )]
    source: Option<String>,

    /// Destination. This may be a file or directory. It may be local or remote
    /// (specified as HOST:DESTINATION, or simply HOST: to copy to your home directory there).
    #[arg(
        conflicts_with_all(MODE_OPTIONS),
        required = true,
        value_name = "DESTINATION"
    )]
    destination: Option<String>,
}

impl Cli {
    pub(crate) fn address_family(&self) -> AddressFamily {
        if self.ipv4 {
            AddressFamily::IPv4
        } else if self.ipv6 {
            AddressFamily::IPv6
        } else {
            AddressFamily::Any
        }
    }
}

/// An unpicked file source or destination specified by the user
#[derive(Debug, Clone)]
pub struct FileSpec {
    pub host: Option<String>,
    pub filename: String,
}

impl FromStr for FileSpec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with('[') {
            // Assume raw IPv6 address [1:2:3::4]:File
            todo!("Raw IPv6 addresses are not yet implemented");
        } else {
            // Host:File or raw IPv4 address 1.2.3.4:File; or just a filename
            match s.split_once(':') {
                Some((host, filename)) => Ok(Self {
                    host: Some(host.to_string()),
                    filename: filename.to_string(),
                }),
                None => Ok(Self {
                    host: None,
                    filename: s.to_owned(),
                }),
            }
        }
    }
}

/// Wrapper type for ClientArgs after we've thought about them
#[derive(Debug, Clone)]
pub(crate) struct ProcessedArgs {
    pub(crate) source: FileSpec,
    pub(crate) destination: FileSpec,
    pub(crate) original: Cli,
}

impl ProcessedArgs {
    pub fn remote_host(&self) -> &str {
        self.source
            .host
            .as_ref()
            .unwrap_or_else(|| self.destination.host.as_ref().unwrap())
    }
}

impl TryFrom<Cli> for ProcessedArgs {
    type Error = anyhow::Error;

    fn try_from(args: Cli) -> Result<Self, Self::Error> {
        let source = match &args.source {
            Some(s) => FileSpec::from_str(s)?,
            None => anyhow::bail!("Source and destination are required"),
        };
        let destination = match &args.destination {
            Some(d) => FileSpec::from_str(d)?,
            None => anyhow::bail!("Destination is required"),
        };

        if (source.host.is_none() && destination.host.is_none())
            || (source.host.is_some() && destination.host.is_some())
        {
            anyhow::bail!("One file argument must be remote");
        }
        Ok(Self {
            source,
            destination,
            original: args,
        })
    }
}

pub fn cli_main() -> anyhow::Result<ExitCode> {
    let args = Cli::parse();
    if args.help_buffers {
        // One day we might make this a function of the remote host.
        let send_window = transport::SEND_BUFFER_SIZE;
        let recv_window =
            transport::practical_receive_window_for(*args.bandwidth, args.rtt)? as usize;
        os::print_udp_buffer_size_help_message(recv_window, send_window);
        return Ok(ExitCode::SUCCESS);
    }
    if args.server {
        anyhow::bail!("Not yet implemented");
    }

    let progress = MultiProgress::new(); // This writes to stderr
    let trace_level = match args.debug {
        true => "trace",
        false => match args.quiet {
            true => "error",
            false => "info",
        },
    };
    setup_tracing(trace_level, Some(&progress), &args.log_file)
        .inspect_err(|e| eprintln!("{e:?}"))?;

    client::client_main(args, progress)
        .inspect_err(|e| tracing::error!("{e}"))
        .or_else(|_| Ok(false))
        .map(|success| match success {
            true => ExitCode::SUCCESS,
            false => ExitCode::FAILURE,
        })
}
