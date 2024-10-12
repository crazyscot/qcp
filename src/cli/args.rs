// QCP top-level command-line arguments
// (c) 2024 Ross Younger

use crate::{
    build_info,
    client::job::FileSpec,
    util::{AddressFamily, PortRange},
};
use clap::Parser;
use tokio::time::Duration;

/// Options that switch us into another mode i.e. which don't require source/destination arguments
const MODE_OPTIONS: &[&str] = &["server", "help_buffers"];

fn parse_duration(arg: &str) -> Result<std::time::Duration, std::num::ParseIntError> {
    let seconds = arg.parse()?;
    Ok(std::time::Duration::from_secs(seconds))
}

#[derive(Debug, Parser, Clone)]
#[command(
    author,
    version(build_info::GIT_VERSION),
    about,
    before_help = "e.g.   qcp some/file my-server:some-directory/",
    infer_long_args(true)
)]
#[command(help_template(
    "\
{name} version {version}
{about-with-newline}
{usage-heading} {usage}
{before-help}
{all-args}{after-help}
"
))]
#[command(styles=crate::styles::get())]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct CliArgs {
    // MODE SELECTION ======================================================================
    /// Operates in server mode.
    ///
    /// This is what we run on the remote machine; it is not
    /// intended for interactive use.
    #[arg(
        long, help_heading("Modes"), hide = true,
        conflicts_with_all(["help_buffers", "quiet", "statistics", "timeout", "ipv4", "ipv6", "remote_debug", "profile", "source", "destination", "ssh", "ssh_opt", "remote_port"])
    )]
    pub server: bool,

    // CLIENT-ONLY OPTIONS =================================================================
    /// Quiet mode
    ///
    /// Switches off progress display and statistics; reports only errors
    #[arg(short, long, action, conflicts_with("debug"))]
    pub quiet: bool,

    /// Outputs additional transfer statistics
    #[arg(short = 's', long, alias("stats"), action, conflicts_with("quiet"))]
    pub statistics: bool,

    /// Connection timeout for the QUIC endpoint.
    ///
    /// This needs to be long enough for your network connection, but short enough to provide
    /// a timely indication that UDP may be blocked.
    #[arg(short, long, default_value("5"), value_name("sec"), value_parser=parse_duration)]
    pub timeout: Duration,

    /// Forces IPv4 connection [default: autodetect]
    #[arg(short = '4', long, action, help_heading("Connection"))]
    pub ipv4: bool,
    /// Forces IPv6 connection [default: autodetect]
    #[arg(
        short = '6',
        long,
        action,
        conflicts_with("ipv4"),
        help_heading("Connection")
    )]
    pub ipv6: bool,

    /// Specifies the ssh client program to use
    #[arg(long, default_value("ssh"), help_heading("Connection"))]
    pub ssh: String,

    /// Provides an additional option or argument to pass to the ssh client.
    ///
    /// Note that you must repeat `-S` for each.
    /// For example, to pass `-i /dev/null` to ssh, specify: `-S -i -S /dev/null`
    #[arg(
        short = 'S',
        action,
        value_name("ssh-option"),
        allow_hyphen_values(true),
        help_heading("Connection")
    )]
    pub ssh_opt: Vec<String>,

    /// Outputs additional information about kernel UDP buffer sizes and platform-specific tips
    #[arg(long, action, help_heading("Network tuning"), display_order(50))]
    pub help_buffers: bool,

    // CLIENT OR SERVER
    /// Uses the given UDP port or range on the local endpoint.
    ///
    /// This can be useful when there is a firewall between the endpoints.
    #[arg(short = 'p', long, value_name("M-N"), help_heading("Connection"))]
    pub port: Option<PortRange>,

    // CLIENT ONLY
    /// Uses the given UDP port or range on the remote endpoint.
    ///
    /// This can be useful when there is a firewall between the endpoints.
    #[arg(short = 'P', long, value_name("M-N"), help_heading("Connection"))]
    pub remote_port: Option<PortRange>,

    // CLIENT DEBUG ----------------------------
    /// Enable detailed debug output
    ///
    /// This has the same effect as setting `RUST_LOG=qcp=debug` in the environment.
    /// If present, `RUST_LOG` overrides this option.
    #[arg(short, long, action, help_heading("Debug"))]
    pub debug: bool,
    /// Enables detailed debug output from the remote endpoint
    #[arg(long, action, help_heading("Debug"))]
    pub remote_debug: bool,
    /// Prints timing profile data after completion
    #[arg(long, action, help_heading("Debug"))]
    pub profile: bool,
    /// Log to a file
    ///
    /// By default the log receives everything printed to stderr.
    /// To override this behaviour, set the environment variable `RUST_LOG_FILE_DETAIL` (same semantics as `RUST_LOG`).
    #[arg(short('l'), long, action, help_heading("Debug"), value_name("FILE"))]
    pub log_file: Option<String>,

    // NETWORK OPTIONS =====================================================================
    #[command(flatten)]
    pub bandwidth: crate::transport::BandwidthParams,

    // POSITIONAL ARGUMENTS ================================================================
    /// The source file. This may be a local filename, or remote specified as HOST:FILE or USER@HOST:FILE.
    ///
    /// Exactly one of source and destination must be remote.
    #[arg(
        conflicts_with_all(MODE_OPTIONS),
        required = true,
        value_name = "SOURCE"
    )]
    pub source: Option<FileSpec>,

    /// Destination. This may be a file or directory. It may be local or remote.
    ///
    /// If remote, specify as HOST:DESTINATION or USER@HOST:DESTINATION; or simply HOST: or USER@HOST: to copy to your home directory there.
    ///
    /// Exactly one of source and destination must be remote.
    #[arg(
        conflicts_with_all(MODE_OPTIONS),
        required = true,
        value_name = "DESTINATION"
    )]
    pub destination: Option<FileSpec>,
}

impl CliArgs {
    pub(crate) fn address_family(&self) -> AddressFamily {
        if self.ipv4 {
            AddressFamily::IPv4
        } else if self.ipv6 {
            AddressFamily::IPv6
        } else {
            AddressFamily::Any
        }
    }

    pub(crate) fn remote_user_host(&self) -> anyhow::Result<&str> {
        let src = self.source.as_ref().ok_or(anyhow::anyhow!(
            "both source and destination must be specified"
        ))?;
        let dest = self.destination.as_ref().ok_or(anyhow::anyhow!(
            "both source and destination must be specified"
        ))?;
        Ok(src
            .host
            .as_ref()
            .unwrap_or_else(|| dest.host.as_ref().unwrap()))
    }

    pub(crate) fn remote_host(&self) -> anyhow::Result<&str> {
        let user_host = self.remote_user_host()?;
        // It might be user@host, or it might be just the hostname or IP.
        let (_, host) = user_host.split_once('@').unwrap_or(("", user_host));
        Ok(host)
    }
}
