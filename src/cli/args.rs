// QCP top-level command-line arguments
// (c) 2024 Ross Younger

use clap::{ArgAction::SetTrue, Args as _, FromArgMatches as _, Parser};

use crate::{config::Manager, util::AddressFamily};

/// Options that switch us into another mode i.e. which don't require source/destination arguments
pub(crate) const MODE_OPTIONS: &[&str] = &["server", "help_buffers", "config_files", "show_config"];

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
#[command(styles=super::styles::CLAP_STYLES)]
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
            "quiet", "statistics", "remote_debug", "profile",
            "ssh", "ssh_opt", "remote_port",
            "source", "destination",
        ])
    )]
    pub server: bool,

    /// Outputs the configuration, then exits.
    ///
    /// If a remote `SOURCE` or `DESTINATION` argument is given, outputs the configuration we would use
    /// for operations to that host.
    ///
    /// If not, outputs only global settings from configuration, which may be overridden in
    /// `Host` blocks in configuration files.
    ///
    #[arg(long, help_heading("Configuration"))]
    pub show_config: bool,
    /// Outputs the paths to configuration file(s), then exits
    #[arg(long, help_heading("Configuration"))]
    pub config_files: bool,

    /// Outputs additional information about kernel UDP buffer sizes and platform-specific tips
    #[arg(long, action, help_heading("Network tuning"), display_order(50))]
    pub help_buffers: bool,

    // CONFIGURABLE OPTIONS ================================================================
    #[command(flatten)]
    pub config: crate::config::Configuration_Optional,

    // CLIENT-SIDE NON-CONFIGURABLE OPTIONS ================================================
    // (including positional arguments!)
    #[command(flatten)]
    pub client_params: crate::client::Parameters,

    /// Convenience alias for `--address-family 4`
    // this is actioned by our custom parser
    #[arg(
        short = '4',
        help_heading("Connection"),
        group("ip address"),
        action(SetTrue)
    )]
    pub ipv4_alias__: bool,
    /// Convenience alias for `--address-family 6`
    // this is actioned by our custom parser
    #[arg(
        short = '6',
        help_heading("Connection"),
        group("ip address"),
        action(SetTrue)
    )]
    pub ipv6_alias__: bool,
}

impl CliArgs {
    /// Sets up and executes our parser
    pub(crate) fn custom_parse() -> Self {
        let cli = clap::Command::new(clap::crate_name!());
        let cli = CliArgs::augment_args(cli).version(crate::version::short());
        let mut args =
            CliArgs::from_arg_matches(&cli.get_matches_from(std::env::args_os())).unwrap();
        // Custom logic: '-4' and '-6' convenience aliases
        if args.ipv4_alias__ {
            args.config.address_family = Some(AddressFamily::Inet);
        } else if args.ipv6_alias__ {
            args.config.address_family = Some(AddressFamily::Inet6);
        }
        args
    }
}

impl TryFrom<&CliArgs> for Manager {
    type Error = anyhow::Error;

    fn try_from(value: &CliArgs) -> Result<Self, Self::Error> {
        let host = value.client_params.remote_host_lossy()?;

        let mut mgr = Manager::standard(host.as_deref());
        mgr.merge_provider(&value.config);
        Ok(mgr)
    }
}
