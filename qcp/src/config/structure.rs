//! Configuration structure
// (c) 2024 Ross Younger

use std::sync::LazyLock;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, builder::TypedValueParser as _};
use engineering_repr::{EngineeringQuantity, EngineeringRepr};
use human_repr::{HumanCount as _, HumanDuration as _};
use serde::{Deserialize, Serialize};
use struct_field_names_as_array::FieldNamesAsSlice;

use crate::{
    cli::styles::{ColourMode, RESET, info},
    protocol::control::CongestionController,
    util::{
        AddressFamily, PortRange, TimeFormat, VecOrString, derive_deftly_template_Optionalify,
        serialization::SerializeAsString,
    },
};

use derive_deftly::Deftly;

/// Minimum bandwidth we will accept in either direction.
/// You have to have a limit somewhere; zero doesn't work. So I chose 1200 baud ...
pub(crate) const MINIMUM_BANDWIDTH: u64 = 150;

/// The set of configurable options supported by qcp.
///
/// **IMPORTANT:** The configurations of the server and client are combined at runtime.
/// See [`combine_bandwidth_configurations`](crate::transport::combine_bandwidth_configurations) for details.
///
/// ### Configuration files
///
/// These fields may be specified in any order. In configuration files, the field names are case insensitive and we
/// ignore hyphens or underscores.
/// In other words, you can use upper case, lower case, camel case, mixed case, shouty snake case, kebab case, train case, whatever you like.
///
/// [More details about the configuration mechanism](crate::config).
///
/// ### Command line
///
/// All configurable options may be used on the command line. There, they must be specified in kebab-case.
///
/// ### Developer notes
/// There is no `default()`.
/// You can access qcp's hard-wired configuration defaults through [`Configuration::system_default()`].
///
/// This structure uses the [Optionalify](derive_deftly_template_Optionalify) deftly macro to automatically
/// define the `Configuration_Optional` struct, which is the same but has all members of type `Option<whatever>`.
/// This is the magic that lets us use the same underlying struct for CLI and saved configuration files:
/// the CLI uses the `_Optional` version , with everything defaulting to `None`.
/// The result is that wherever the user does not provide a value, values read from lower priority sources
/// (configuration files and system defaults) obtain.
///
// Maintainer note: None of the members of this struct should be Option<anything>. That leads to sunspots in the CLI and strange warts (Some(Some(foo))).
#[derive(Deftly)]
#[derive_deftly(Optionalify)]
#[deftly(visibility = "pub(crate)")]
#[derive(Debug, Clone, PartialEq, Parser, Deserialize, Serialize, FieldNamesAsSlice)]
pub struct Configuration {
    // TRANSPORT PARAMETERS ============================================================================
    // System bandwidth, UDP ports, timeout.
    /// The maximum network bandwidth we expect receiving data FROM the remote system.
    /// [default: 12.5M]
    ///
    /// This parameter is always interpreted as the **local** bandwidth, whether operating in client or server mode.
    ///
    /// This may be specified directly as a number, or as an SI quantity
    /// like `10M` or `256k`. **Note that this is described in BYTES, not bits**;
    /// if (for example) you expect to fill a 1Gbit ethernet connection,
    /// 125M might be a suitable setting.
    ///
    #[arg(
        short('b'),
        long,
        alias("rx-bw"),
        help_heading("Network tuning"),
        display_order(1),
        value_name = "bytes"
    )]
    pub rx: EngineeringQuantity<u64>,
    /// The maximum network bandwidth we expect sending data TO the remote system,
    /// if it is different from the bandwidth FROM the system.
    /// (For example, when you are connected via an asymmetric last-mile DSL or fibre profile.)
    ///
    /// This parameter is always interpreted as the **local** bandwidth, whether operating in client or server mode.
    ///
    /// If not specified or 0, uses the value of `rx`.
    #[arg(
        short('B'),
        long,
        alias("tx-bw"),
        help_heading("Network tuning"),
        display_order(1),
        value_name = "bytes"
    )]
    pub tx: EngineeringQuantity<u64>,

    /// The expected network Round Trip time to the target system, in milliseconds.
    /// [default: 300]
    #[arg(
        short('r'),
        long,
        help_heading("Network tuning"),
        display_order(10),
        value_name("ms")
    )]
    pub rtt: u16,

    /// Specifies the congestion control algorithm to use.
    /// [default: cubic]
    #[arg(
        long,
        action,
        value_name = "algorithm",
        value_parser(clap::builder::EnumValueParser::<CongestionController>::new().map(SerializeAsString)), /* whee, this was fun to figure out :-) */
        help_heading("Advanced network tuning"),
        display_order(0)
    )]
    pub congestion: SerializeAsString<CongestionController>,

    /// _(Network wizards only!)_
    /// The initial value for the sending congestion control window, in bytes.
    /// If unspecified, the active congestion control algorithm decides.
    ///
    /// This may be specified directly as a number, or as an SI quantity like `10k`.
    ///
    /// _Setting this value too high reduces performance!_
    #[arg(
        long,
        help_heading("Advanced network tuning"),
        value_name = "bytes",
        alias("cwnd"),
        display_order(0)
    )]
    pub initial_congestion_window: EngineeringQuantity<u64>,

    /// Uses the given UDP port or range on the **local** endpoint.
    /// This can be useful when there is a firewall between the endpoints.
    ///
    /// For example: `12345`, `20000-20100`
    ///
    /// If unspecified, uses any available UDP port.
    #[arg(
        short = 'p',
        long,
        value_name("M-N"),
        help_heading("Connection"),
        display_order(0)
    )]
    pub port: PortRange,

    /// Connection timeout for the QUIC endpoints [seconds; default 5]
    ///
    /// This needs to be long enough for your network connection, but short enough to provide
    /// a timely indication that UDP may be blocked.
    #[arg(
        short,
        long,
        value_name("sec"),
        help_heading("Connection"),
        display_order(0)
    )]
    pub timeout: u16,

    // CLIENT OPTIONS ==================================================================================
    /// Forces use of a particular IP version when connecting to the remote. [default: any]
    ///
    // (see also [CliArgs::ipv4_alias__] and [CliArgs::ipv6_alias__])
    #[arg(
        long,
        help_heading("Connection"),
        group("ip address"),
        //value_parser(clap::builder::EnumValueParser::<AddressFamily>::new().map(AddressFamily::from)),
        display_order(0)
    )]
    pub address_family: AddressFamily,

    /// Specifies the ssh client program to use [default: `ssh`]
    #[arg(
        long,
        help_heading("Connection"),
        display_order(0),
        value_name("ssh-client")
    )]
    pub ssh: String,

    /// Provides an additional option or argument to pass to the ssh client. [default: none]
    ///
    /// **On the command line** you must repeat `-S` for each argument.
    /// For example, to pass `-i /dev/null` to ssh, specify: `-S -i -S /dev/null`
    #[arg(
        short = 'S',
        value_name("ssh-option"),
        allow_hyphen_values(true),
        help_heading("Connection"),
        value_parser(clap::value_parser!(String)),
        display_order(0)
    )]
    pub ssh_options: VecOrString,

    /// Uses the given UDP port or range on the **remote** endpoint.
    /// This can be useful when there is a firewall between the endpoints.
    ///
    /// For example: `12345`, `20000-20100`
    ///
    /// If unspecified, uses any available UDP port.
    #[arg(
        short = 'P',
        long,
        value_name("M-N"),
        help_heading("Connection"),
        display_order(0)
    )]
    pub remote_port: PortRange,

    /// Specifies the user on the remote machine to connect as.
    ///
    /// This is functionally the same as specifying a remote filename `user@host:file`.
    /// If unspecified, we leave it up to ssh to determine.
    #[arg(
        short = 'l',
        long,
        value_name("login_name"),
        help_heading("Connection"),
        display_order(0)
    )]
    pub remote_user: String,

    /// Specifies the time format to use when printing messages to the console or to file
    /// [default: local]
    #[arg(
        short = 'T',
        long,
        value_name("FORMAT"),
        help_heading("Output"),
        next_line_help(true),
        display_order(0)
    )]
    pub time_format: TimeFormat,

    /// Alternative ssh config file(s)
    ///
    /// By default, qcp reads your user and system ssh config files to look for Hostname aliases.
    /// In some cases the logic in qcp may not read them successfully; this is an escape hatch,
    /// allowing you to specify one or more alternative files to read instead (which may be empty,
    /// nonexistent or /dev/null).
    ///
    /// This option is really intended to be used in a qcp configuration file.
    /// On the command line, you can repeat `--ssh-config file` as many times as needed.
    #[arg(long, value_name("FILE"), help_heading("Connection"), display_order(0), value_parser(clap::value_parser!(String)))]
    pub ssh_config: VecOrString,

    /// Ssh subsystem mode
    ///
    /// This mode causes qcp to run `ssh <host> -s qcp` instead of `ssh <host> qcp --server`.
    ///
    /// This is useful where the remote system has a locked-down `PATH` and the qcp binary
    /// is not resident on that `PATH`.
    /// The remote system sshd has to be configured with a line like this:
    ///
    /// `Subsystem qcp /usr/local/bin/qcp --server`
    #[arg(
        long,
        alias("subsystem"),
        default_missing_value("true"), // required for a bool in Configuration, along with num_args and require_equals
        num_args(0..=1),
        require_equals(true),
        help_heading("Connection"),
        display_order(0)
    )]
    pub ssh_subsystem: bool,

    /// Colour mode for console output (default: auto)
    ///
    /// Passing `--color` without a value is equivalent to `--color always`.
    ///
    /// Note that color configuration is not shared with the remote system, so the color output
    /// from the remote system (log messages, remote-config) will be coloured per the
    /// config file on the remote system.
    ///
    /// qcp also supports the `CLICOLOR`, `CLICOLOR_FORCE` and `NO_COLOR` environment variables.
    /// See [https://bixense.com/clicolors/](https://bixense.com/clicolors/) for more details.
    ///
    /// CLI options take precedence over the configuration file, which takes precedence over environment variables.
    #[arg(
        long,
        alias("colour"),
        default_missing_value("always"), // to support `--color`
        num_args(0..=1),
        //value_parser(clap::builder::EnumValueParser::<ColourMode>::new().map(ColourMode::from)),
        value_name("mode")
    )]
    pub color: ColourMode,
}

static SYSTEM_DEFAULT_CONFIG: LazyLock<Configuration> = LazyLock::new(|| Configuration {
    // Transport
    rx: 12_500_000u64.into(),
    tx: 0u64.into(),
    rtt: 300,
    congestion: CongestionController::Cubic.into(),
    initial_congestion_window: 0u64.into(),
    port: PortRange::default(),
    timeout: 5,
    // Client
    address_family: AddressFamily::Any,
    ssh: "ssh".into(),
    ssh_options: VecOrString::default(),
    remote_port: PortRange::default(),
    remote_user: String::new(),
    time_format: TimeFormat::Local,
    ssh_config: VecOrString::default(),
    ssh_subsystem: false,
    color: ColourMode::Auto,
});

impl Configuration {
    /// Computes the theoretical bandwidth-delay product for outbound data
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn bandwidth_delay_product_tx(&self) -> u64 {
        self.tx() * u64::from(self.rtt) / 1000
    }
    /// Computes the theoretical bandwidth-delay product for inbound data
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn bandwidth_delay_product_rx(&self) -> u64 {
        self.rx() * u64::from(self.rtt) / 1000
    }
    #[must_use]
    /// Receive bandwidth (accessor)
    pub fn rx(&self) -> u64 {
        self.rx.into()
    }
    #[must_use]
    /// Transmit bandwidth (accessor)
    pub fn tx(&self) -> u64 {
        match u64::from(self.tx) {
            0 => self.rx(),
            tx => tx,
        }
    }
    /// RTT accessor as Duration
    #[must_use]
    pub fn rtt_duration(&self) -> Duration {
        Duration::from_millis(u64::from(self.rtt))
    }

    /// UDP kernel sending buffer size to use
    #[must_use]
    pub fn send_buffer() -> u64 {
        // UDP kernel buffers of 2MB have proven sufficient to get close to line speed on a 300Mbit downlink with 300ms RTT.
        2_097_152
    }
    /// UDP kernel receive buffer size to use
    #[must_use]
    pub fn recv_buffer() -> u64 {
        // UDP kernel buffers of 2MB have proven sufficient to get close to line speed on a 300Mbit downlink with 300ms RTT.
        2_097_152
    }

    /// QUIC receive window
    #[must_use]
    pub fn recv_window(&self) -> u64 {
        // The theoretical in-flight limit appears to be sufficient
        self.bandwidth_delay_product_rx()
    }

    /// QUIC send window
    #[must_use]
    pub fn send_window(&self) -> u64 {
        // There might be random added latency en route, so provide for a larger send window than theoretical.
        2 * self.bandwidth_delay_product_tx()
    }

    /// Accessor for `timeout`, as a Duration
    #[must_use]
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.timeout.into())
    }

    /// Formats the transport-related options for display
    #[must_use]
    pub fn format_transport_config(&self) -> String {
        let iwind = match u64::from(self.initial_congestion_window) {
            0 => "<default>".to_string(),
            s => s.human_count_bytes().to_string(),
        };
        let (tx, rx) = (self.tx(), self.rx());
        format!(
            "rx {rx} ({rxbits}), tx {tx} ({txbits}), rtt {rtt}, congestion algorithm {congestion} with initial window {iwind}",
            tx = tx.human_count_bytes(),
            txbits = (tx * 8).human_count("bit"),
            rx = rx.human_count_bytes(),
            rxbits = (rx * 8).human_count("bit"),
            rtt = self.rtt_duration().human_duration(),
            congestion = self.congestion,
        )
    }

    /// Returns the system default settings
    #[must_use]
    pub fn system_default() -> &'static Self {
        &SYSTEM_DEFAULT_CONFIG
    }
}

// VALIDATION ------------------------------------------------------------

/// Data needed by [`Validatable::try_validate()`]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValidationData {
    rtt: u16,
    rx: u64,
    tx: u64,
}

impl Configuration {
    fn validation_data(&self) -> ValidationData {
        ValidationData {
            rtt: self.rtt,
            rx: self.rx(),
            tx: self.tx(),
        }
    }

    /// Performs additional validation checks on a configuration object
    pub(crate) fn try_validate(&self) -> Result<()> {
        let data = self.validation_data();

        let rtt = data.rtt;
        let rx = data.rx;
        if rx < MINIMUM_BANDWIDTH {
            anyhow::bail!(
                "The receive bandwidth ({INFO}rx {val}{RESET}B) is too small; it must be at least {min}",
                val = rx.to_eng(0),
                min = MINIMUM_BANDWIDTH.to_eng(3),
                INFO = info()
            );
        }
        if rx.checked_mul(rtt.into()).is_none() {
            anyhow::bail!(
                "The receive bandwidth delay product calculation ({INFO}rx {val}{RESET}B x {INFO}rtt {rtt}{RESET}ms) overflowed",
                val = rx.to_eng(0),
                INFO = info()
            );
        }

        let tx = data.tx;
        if tx != 0 && tx < MINIMUM_BANDWIDTH {
            anyhow::bail!(
                "The transmit bandwidth ({INFO}tx {val}{RESET}B) is too small; it must be at least {min}",
                val = tx.to_eng(0),
                min = MINIMUM_BANDWIDTH.to_eng(3),
                INFO = info(),
            );
        }
        if tx.checked_mul(rtt.into()).is_none() {
            anyhow::bail!(
                "The transmit bandwidth delay product calculation ({INFO}tx {val}{RESET}B x {INFO}rtt {rtt}{RESET}ms) overflowed",
                val = tx.to_eng(0),
                INFO = info(),
            );
        }
        Ok(())
    }

    /// Performs additional validation checks on the configuration.
    pub(crate) fn validate(self) -> Result<Self>
    where
        Self: std::marker::Sized,
    {
        self.try_validate()?;
        Ok(self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::config::Manager;

    use super::SYSTEM_DEFAULT_CONFIG;

    #[test]
    fn flattened() {
        let v = SYSTEM_DEFAULT_CONFIG.clone();
        let j = serde_json::to_string(&v).unwrap();
        let d = json::parse(&j).unwrap();
        assert!(!d.has_key("bw"));
        assert!(d.has_key("rtt"));
    }

    #[test]
    fn accessors() {
        let mut cfg = SYSTEM_DEFAULT_CONFIG.clone();
        cfg.rtt = 123;
        assert_eq!(cfg.rtt_duration().as_millis(), 123);
        cfg.timeout = 4;
        assert_eq!(cfg.timeout_duration().as_secs(), 4);

        let s = cfg.format_transport_config();
        assert!(s.contains("rx 12.5MB (100Mbit)"));
        assert!(s.contains("rtt 123ms"));
        assert!(s.contains("congestion algorithm cubic with initial window <default>"));

        cfg.initial_congestion_window = 1000u64.into();
        let s = cfg.format_transport_config();
        assert!(s.contains("congestion algorithm cubic with initial window 1kB"));
    }

    #[test]
    fn validate() {
        let mut cfg = SYSTEM_DEFAULT_CONFIG.clone();
        assert!(cfg.try_validate().is_ok());

        // rx too small
        cfg.rx = 1u64.into();
        let err = cfg.try_validate().unwrap_err();
        assert_eq!(
            console::strip_ansi_codes(&err.to_string()),
            "The receive bandwidth (rx 1B) is too small; it must be at least 150"
        );

        // tx too small
        cfg = SYSTEM_DEFAULT_CONFIG.clone();
        cfg.tx = 1u64.into();
        let err = cfg.try_validate().unwrap_err();
        assert_eq!(
            console::strip_ansi_codes(&err.to_string()),
            "The transmit bandwidth (tx 1B) is too small; it must be at least 150"
        );

        // rx overflow
        cfg = SYSTEM_DEFAULT_CONFIG.clone();
        cfg.rx = u64::MAX.into();
        let err = cfg.try_validate().unwrap_err().to_string();
        let msg = console::strip_ansi_codes(&err);
        assert!(
            msg.contains("The receive bandwidth delay product calculation")
                && msg.contains("overflowed")
        );
        // tx overflow
        cfg = SYSTEM_DEFAULT_CONFIG.clone();
        cfg.tx = u64::MAX.into();
        let err = cfg.try_validate().unwrap_err().to_string();
        let msg = console::strip_ansi_codes(&err);
        assert!(
            msg.contains("The transmit bandwidth delay product calculation")
                && msg.contains("overflowed")
        );
    }

    #[test]
    fn issue_123_validate_default_data() {
        let mgr = Manager::without_files(None);
        let cfg = mgr.get::<super::Configuration>().unwrap();
        let data = cfg.validation_data();
        assert_eq!(data.rtt, SYSTEM_DEFAULT_CONFIG.rtt);
        assert_eq!(data.rx, SYSTEM_DEFAULT_CONFIG.rx());
        assert_eq!(data.tx, SYSTEM_DEFAULT_CONFIG.tx());
    }
}
