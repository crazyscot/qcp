//! Options specific to qcp client-mode
// (c) 2024 Ross Younger

use clap::Parser;

use crate::{protocol::control::ConnectionType, util::PortRange};

/// Configurable options specific to qcp client mode
#[derive(Debug, Parser, Clone)]
pub struct Options {
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

    /// Uses the given UDP port or range on the remote endpoint.
    ///
    /// This can be useful when there is a firewall between the endpoints.
    #[arg(short = 'P', long, value_name("M-N"), help_heading("Connection"))]
    pub remote_port: Option<PortRange>,
}

impl Options {
    pub(crate) fn address_family(&self) -> Option<ConnectionType> {
        if self.ipv4 {
            Some(ConnectionType::Ipv4)
        } else if self.ipv6 {
            Some(ConnectionType::Ipv6)
        } else {
            None
        }
    }
}

#[derive(Debug, Parser, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
/// Optional behaviours for qcp
pub struct Behaviours {
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

    /// Quiet mode
    ///
    /// Switches off progress display and statistics; reports only errors
    #[arg(short, long, action, conflicts_with("debug"))]
    pub quiet: bool,

    /// Outputs additional transfer statistics
    #[arg(short = 's', long, alias("stats"), action, conflicts_with("quiet"))]
    pub statistics: bool,

    /// Enables detailed debug output from the remote endpoint
    /// (this may interfere with transfer speeds)
    #[arg(long, action, help_heading("Debug"))]
    pub remote_debug: bool,

    /// Prints timing profile data after completion
    #[arg(long, action, help_heading("Debug"))]
    pub profile: bool,
}
