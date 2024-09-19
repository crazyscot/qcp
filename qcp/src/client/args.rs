use std::str::FromStr;

// qcp client - command line interface
// (c) 2024 Ross Younger
use crate::build_info;
use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    author,
    version(build_info::GIT_VERSION),
    about,
    long_about = "QUIC file copy utility"
)]
#[command(help_template(
    "\
{before-help}{name} {version}
(c) {author-with-newline}{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}
"
))]
#[command(styles=crate::styles::get())]
/// The arguments we need to set up a client
pub struct ClientArgs {
    /// Enable detailed debug output
    #[arg(short, long, action)]
    pub debug: bool,
    /// Quiet mode (reduced chatter)
    #[arg(short, long, action)]
    pub quiet: bool,
    /// Connection timeout (seconds)
    #[arg(short, long, default_value("10"))]
    pub timeout: u16,
    /// Enables server debug output
    #[arg(short, long, action)]
    pub server_debug: bool,

    // Positional arguments
    #[arg()]
    pub source: String,
    #[arg()]
    pub destination: String,
    // TODO support multiple sources, cp-like?
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
pub struct ProcessedArgs<'a> {
    pub source: FileSpec,
    pub destination: FileSpec,
    pub original: &'a ClientArgs,
}

impl<'a> ProcessedArgs<'_> {
    pub fn remote_host(&'a self) -> &'a str {
        self.source
            .host
            .as_ref()
            .unwrap_or_else(|| self.destination.host.as_ref().unwrap())
    }
}

impl<'a> TryFrom<&'a ClientArgs> for ProcessedArgs<'a> {
    type Error = anyhow::Error;

    fn try_from(args: &'a ClientArgs) -> Result<Self, Self::Error> {
        let source = FileSpec::from_str(&args.source)?;
        let destination = FileSpec::from_str(&args.destination)?;
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
