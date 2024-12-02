// QCP top-level command-line arguments
// (c) 2024 Ross Younger

use clap::{Args as _, FromArgMatches as _, Parser};

use crate::{client::CopyJobSpec, config::Manager};

/// Options that switch us into another mode i.e. which don't require source/destination arguments
pub(crate) const MODE_OPTIONS: &[&str] = &["server", "help_buffers", "show_config", "config_files"];

#[derive(Debug, Parser, Clone)]
#[command(
    author,
    // we set short/long version strings explicitly, see custom_parse()
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
#[command(styles=super::styles::get())]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct CliArgs {
    // MODE SELECTION ======================================================================
    /// Operates in server mode.
    ///
    /// This is what we run on the remote machine; it is not
    /// intended for interactive use.
    #[arg(
        long, help_heading("Modes"), hide = true,
        conflicts_with_all([
            "help_buffers", "show_config", "config_files",
            "ipv4", "ipv6",
            "quiet", "statistics", "remote_debug", "profile",
            "ssh", "ssh_opt", "remote_port",
            "source", "destination",
        ])
    )]
    pub server: bool,

    /// Outputs the configuration, then exits
    #[arg(long, help_heading("Configuration"))]
    pub show_config: bool,
    /// Outputs the paths to configuration file(s), then exits
    #[arg(long, help_heading("Configuration"))]
    pub config_files: bool,

    /// Outputs additional information about kernel UDP buffer sizes and platform-specific tips
    #[arg(long, action, help_heading("Network tuning"), display_order(50))]
    pub help_buffers: bool,

    // CLIENT-SIDE CONFIGURABLE OPTIONS ====================================================
    #[command(flatten)]
    pub client: crate::client::ClientConfiguration_Optional,

    // NETWORK OPTIONS =====================================================================
    #[command(flatten)]
    pub bandwidth: crate::transport::BandwidthParams_Optional,

    #[command(flatten)]
    pub quic: crate::transport::QuicParams_Optional,

    // CLIENT-SIDE NON-CONFIGURABLE OPTIONS ================================================
    // (including positional arguments!)
    #[command(flatten)]
    pub parameters: crate::client::Parameters,
}

impl CliArgs {
    /// Sets up and executes our parser
    pub(crate) fn custom_parse() -> Self {
        let cli = clap::Command::new(clap::crate_name!());
        let cli = CliArgs::augment_args(cli).version(crate::version::short());
        CliArgs::from_arg_matches(&cli.get_matches_from(std::env::args_os())).unwrap()
    }
}

impl From<&CliArgs> for Manager {
    /// Merge options from the CLI into the structure.
    /// Any new option packs (_Optional structs) need to be added here.
    fn from(value: &CliArgs) -> Self {
        let mut mgr = Manager::new();
        mgr.merge_provider(value.bandwidth);
        mgr.merge_provider(value.quic);
        mgr.merge_provider(&value.client);
        mgr
    }
}

impl TryFrom<&CliArgs> for CopyJobSpec {
    type Error = anyhow::Error;

    fn try_from(args: &CliArgs) -> Result<Self, Self::Error> {
        CopyJobSpec::try_from(&args.parameters)
    }
}
