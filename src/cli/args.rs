// QCP top-level command-line arguments
// (c) 2024 Ross Younger

use crate::build_info;
use clap::Parser;

/// Options that switch us into another mode i.e. which don't require source/destination arguments
pub(crate) const MODE_OPTIONS: &[&str] = &["server", "help_buffers"];

#[derive(Debug, Parser, Clone)]
#[command(
    author,
    version,
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
#[command(styles=super::styles::get())]
pub(crate) struct CliArgs {
    // MODE SELECTION ======================================================================
    /// Operates in server mode.
    ///
    /// This is what we run on the remote machine; it is not
    /// intended for interactive use.
    #[arg(
        long, help_heading("Modes"), hide = true,
        conflicts_with_all(["help_buffers", "quiet", "statistics", "ipv4", "ipv6", "remote_debug", "profile", "source", "destination", "ssh", "ssh_opt", "remote_port"])
    )]
    pub server: bool,

    /// Outputs additional information about kernel UDP buffer sizes and platform-specific tips
    #[arg(long, action, help_heading("Network tuning"), display_order(50))]
    pub help_buffers: bool,

    // CLIENT-ONLY OPTIONS =================================================================
    #[command(flatten)]
    pub client: crate::client::Options,

    // NETWORK OPTIONS =====================================================================
    #[command(flatten)]
    pub bandwidth: crate::transport::BandwidthParams,

    #[command(flatten)]
    pub quic: crate::transport::QuicParams,
    // DEBUG OPTIONS =======================================================================
    /// Enable detailed debug output
    ///
    /// This has the same effect as setting `RUST_LOG=qcp=debug` in the environment.
    /// If present, `RUST_LOG` overrides this option.
    #[arg(short, long, action, help_heading("Debug"))]
    pub debug: bool,
    /// Log to a file
    ///
    /// By default the log receives everything printed to stderr.
    /// To override this behaviour, set the environment variable `RUST_LOG_FILE_DETAIL` (same semantics as `RUST_LOG`).
    #[arg(short('l'), long, action, help_heading("Debug"), value_name("FILE"))]
    pub log_file: Option<String>,
    //
    // ======================================================================================
    //
    // N.B. ClientOptions has positional arguments!
}
