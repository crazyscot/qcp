//! Options specific to qcp client-mode
// (c) 2024 Ross Younger

use clap::Parser;
use derive_deftly::Deftly;
use serde::{Deserialize, Serialize};

use crate::{
    protocol::control::ConnectionType,
    util::{derive_deftly_template_Optionalify, PortRange},
};

/// Configurable options which only make sense in client mode
#[derive(Deftly)]
#[derive_deftly(Optionalify)]
#[deftly(visibility = "pub(crate)")]
#[derive(Clone, Debug, PartialEq, Eq, Parser, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct ClientConfiguration {
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

    /// Specifies the ssh client program to use [default: ssh]
    #[arg(long, help_heading("Connection"))]
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

impl Default for ClientConfiguration {
    fn default() -> Self {
        Self {
            ipv4: false,
            ipv6: false,
            ssh: "ssh".into(),
            ssh_opt: vec![],
            remote_port: None,
        }
    }
}

impl ClientConfiguration {
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
/// Non-configurable client-side parameters
pub struct Parameters {
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
