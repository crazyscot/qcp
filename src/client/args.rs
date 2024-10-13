//! qcp Client parameters
// (c) 2024 Ross Younger

use clap::Parser;

use crate::{protocol::control::ConnectionType, util::PortRange};

use super::job::FileSpec;

/// Options for client mode
#[derive(Debug, Parser, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ClientOptions {
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

    // POSITIONAL ARGUMENTS ================================================================
    /// The source file. This may be a local filename, or remote specified as HOST:FILE or USER@HOST:FILE.
    ///
    /// Exactly one of source and destination must be remote.
    #[arg(
        conflicts_with_all(crate::cli::MODE_OPTIONS),
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
        conflicts_with_all(crate::cli::MODE_OPTIONS),
        required = true,
        value_name = "DESTINATION"
    )]
    pub destination: Option<FileSpec>,
}

impl ClientOptions {
    pub(crate) fn address_family(&self) -> Option<ConnectionType> {
        if self.ipv4 {
            Some(ConnectionType::Ipv4)
        } else if self.ipv6 {
            Some(ConnectionType::Ipv6)
        } else {
            None
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
