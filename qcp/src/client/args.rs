use std::str::FromStr;

// qcp client - command line interface
// (c) 2024 Ross Younger
use crate::{build_info, util::AddressFamily};
use clap::Parser;

#[derive(Debug, Parser, Clone)]
#[command(
    author,
    version(build_info::GIT_VERSION),
    about,
    long_about = "QUIC file copy utility",
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
/// The arguments we need to set up a client
pub struct ClientArgs {
    /// Quiet mode (no statistics or progress, report only errors)
    #[arg(short, long, action)]
    pub quiet: bool,
    /// Connection timeout (seconds)
    #[arg(short, long, default_value("10"))]
    pub timeout: u16,

    /// The file buffer size to use (default 2MB; tune to your needs).
    /// We use a network buffer 4x this size.
    /// Setting the buffer too small will harm performance; too large is inefficient.
    /// See also kernel-buffer-size.
    #[arg(short('b'), long, default_value("2097152"))]
    pub buffer_size: usize,
    /// Forces IPv4 connection (default: autodetect)
    #[arg(short = '4', long, action)]
    pub ipv4: bool,
    /// Forces IPv6 connection (default: autodetect)
    #[arg(short = '6', long, action, conflicts_with("ipv4"))]
    pub ipv6: bool,
    /// Outputs additional transfer statistics
    #[arg(short = 's', long, alias("stats"), action, conflicts_with("quiet"))]
    pub statistics: bool,

    /// The UDP buffer size to request from the operating system kernel.
    /// This should be larger than the file buffer size.
    #[arg(
        short('k'),
        long,
        default_value("7340032" /*7MB*/)
    )]
    pub kernel_buffer_size: usize,

    /// Enable detailed debug output
    #[arg(short, long, action, help_heading("Debug options"))]
    pub debug: bool,
    /// Enables detailed server (remote) debug output
    #[arg(long, action, help_heading("Debug options"))]
    pub remote_debug: bool,
    /// Prints timing profile data after completion
    #[arg(long, action, help_heading("Debug options"))]
    pub profile: bool,

    // Special option (a form of help message)
    /// Outputs a help message about UDP buffer sizes
    #[arg(long, action, hide(true))]
    pub help_socket_bufsize: bool,

    // Positional arguments
    /// Source file. This may be a local filename, or remote specified as HOST:FILE.
    pub source: Option<String>,

    /// Destination. This may be a file or directory. It may be local or remote
    /// (specified as HOST:DESTINATION, or simply HOST: to copy to your home directory there).
    pub destination: Option<String>,
    // SOMEDAY: we might support arbitrarily many positional args, cp-like.
}

impl ClientArgs {
    /// Buffer size to use for network operations
    pub(crate) fn network_buffer_size(&self) -> usize {
        self.buffer_size * 4
    }

    /// Buffer size to use for file operations
    pub(crate) fn file_buffer_size(&self) -> usize {
        self.buffer_size
    }

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
pub struct ProcessedArgs {
    pub source: FileSpec,
    pub destination: FileSpec,
    pub original: ClientArgs,
}

impl ProcessedArgs {
    pub fn remote_host(&self) -> &str {
        self.source
            .host
            .as_ref()
            .unwrap_or_else(|| self.destination.host.as_ref().unwrap())
    }
}

impl TryFrom<ClientArgs> for ProcessedArgs {
    type Error = anyhow::Error;

    fn try_from(args: ClientArgs) -> Result<Self, Self::Error> {
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
