// QCP command-line arguments
// (c) 2024 Ross Younger

use std::str::FromStr;

use crate::{build_info, util::AddressFamily};
use clap::Parser;
use humanize_rs::bytes::Bytes;
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
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct CliArgs {
    // MODE SELECTION ======================================================================
    /// Operates in server mode. This is what we run on the remote machine; it is not
    /// intended for interactive use.
    #[arg(
        long, help_heading("Modes"), hide = true,
        conflicts_with_all(["help_buffers", "quiet", "statistics", "timeout", "ipv4", "ipv6", "remote_debug", "profile"])
    )]
    pub server: bool,

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
    #[arg(short, long, default_value("5"), value_name("seconds"), value_parser=parse_duration)]
    pub timeout: Duration,

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
    /// This can be overridden by setting the environment variable `RUST_LOG_FILE_DETAIL` (same semantics as `RUST_LOG`).
    #[arg(short('l'), long, action, help_heading("Debug options"))]
    pub log_file: Option<String>,

    // TUNING OPTIONS ======================================================================
    /// The maximum network bandwidth we expect to/from the target system.
    /// Along with the initial RTT, this directly affects the buffer sizes used.
    /// This may be specified directly as a number of bytes, or as an SI quantity
    /// e.g. "10M" or "256k". Note that this is described in bytes, not bits;
    /// if (for example) you expect to fill a 1Gbit ethernet connection,
    /// 125M might be a suitable upper limit.
    #[arg(short('b'), long, help_heading("Network tuning"), display_order(50), default_value("12M"), value_name="bytes", value_parser=clap::value_parser!(Bytes))]
    pub bandwidth: Bytes,

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
        hide(true),
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
    pub source: Option<FileSpec>,

    /// Destination. This may be a file or directory. It may be local or remote
    /// (specified as HOST:DESTINATION, or simply HOST: to copy to your home directory there).
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

    pub(crate) fn remote_host(&self) -> anyhow::Result<&str> {
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

    pub(crate) fn bandwidth_bytes(&self) -> anyhow::Result<u64> {
        Ok(self.bandwidth.size().try_into()?)
    }
}

/// An unpacked file source or destination specified by the user
#[derive(Debug, Clone, Default)]
pub(crate) struct FileSpec {
    pub host: Option<String>,
    pub filename: String,
}

impl FromStr for FileSpec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with('[') {
            // Assume raw IPv6 address [1:2:3::4]:File
            match s.split_once("]:") {
                Some((hostish, filename)) => Ok(Self {
                    // lose the leading bracket as well so it can be looked up as if a hostname
                    host: Some(hostish[1..].to_owned()),
                    filename: filename.into(),
                }),
                None => Ok(Self {
                    host: None,
                    filename: s.to_owned(),
                }),
            }
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

/// Convenience struct for the members of the `CliArgs` struct which are required in most circumstances
pub(crate) struct UnpackedArgs {
    pub(crate) source: FileSpec,
    pub(crate) destination: FileSpec,
}

impl TryFrom<&CliArgs> for UnpackedArgs {
    type Error = anyhow::Error;

    fn try_from(args: &CliArgs) -> Result<Self, Self::Error> {
        let source = args
            .source
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("source and destination are required"))?
            .clone();
        let destination = args
            .destination
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("source and destination are required"))?
            .clone();

        if !(source.host.is_none() ^ destination.host.is_none()) {
            anyhow::bail!("One file argument must be remote");
        }

        Ok(Self {
            source,
            destination,
        })
    }
}

#[cfg(test)]
mod test {
    type Res = anyhow::Result<()>;
    use human_repr::HumanCount;

    use super::FileSpec;
    use std::str::FromStr;

    #[test]
    fn filename_no_host() -> Res {
        let fs = FileSpec::from_str("/dir/file")?;
        assert!(fs.host.is_none());
        assert_eq!(fs.filename, "/dir/file");
        Ok(())
    }

    #[test]
    fn host_no_file() -> Res {
        let fs = FileSpec::from_str("host:")?;
        assert_eq!(fs.host.unwrap(), "host");
        assert_eq!(fs.filename, "");
        Ok(())
    }

    #[test]
    fn host_and_file() -> Res {
        let fs = FileSpec::from_str("host:file")?;
        assert_eq!(fs.host.unwrap(), "host");
        assert_eq!(fs.filename, "file");
        Ok(())
    }

    #[test]
    fn bare_ipv4() -> Res {
        let fs = FileSpec::from_str("1.2.3.4:file")?;
        assert_eq!(fs.host.unwrap(), "1.2.3.4");
        assert_eq!(fs.filename, "file");
        Ok(())
    }

    #[test]
    fn bare_ipv6() -> Res {
        let fs = FileSpec::from_str("[1:2:3:4::5]:file")?;
        assert_eq!(fs.host.unwrap(), "1:2:3:4::5");
        assert_eq!(fs.filename, "file");
        Ok(())
    }
    #[test]
    fn bare_ipv6_localhost() -> Res {
        let fs = FileSpec::from_str("[::1]:file")?;
        assert_eq!(fs.host.unwrap(), "::1");
        assert_eq!(fs.filename, "file");
        Ok(())
    }
    #[test]
    fn size_is_kb_not_kib() {
        // same mechanism that clap uses
        use humanize_rs::bytes::Bytes;
        let s = "1k".parse::<Bytes>().unwrap();
        assert_eq!(s.size(), 1000);
    }
    #[test]
    fn human_repr_test() {
        assert_eq!(1000.human_count_bare(), "1k");
    }
}
