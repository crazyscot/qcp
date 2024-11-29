//! Options specific to qcp client-mode
// (c) 2024 Ross Younger

use clap::Parser;

use crate::{protocol::control::ConnectionType, util::PortRange};

/// Options specific to qcp client mode
#[derive(Debug, Parser, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct Options {
    /// Quiet mode
    ///
    /// Switches off progress display and statistics; reports only errors
    #[arg(short, long, action, conflicts_with("debug"))]
    pub quiet: bool,

    /// Outputs additional transfer statistics
    #[arg(short = 's', long, alias("stats"), action, conflicts_with("quiet"))]
    pub statistics: bool,

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

    // CLIENT DEBUG ----------------------------
    /// Enables detailed debug output from the remote endpoint
    #[arg(long, action, help_heading("Debug"))]
    pub remote_debug: bool,
    /// Prints timing profile data after completion
    #[arg(long, action, help_heading("Debug"))]
    pub profile: bool,
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
