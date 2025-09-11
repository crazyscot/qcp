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
    cli::styles::{ColourMode, info, reset},
    protocol::control::{CongestionController, CredentialsType},
    util::{
        AddressFamily, PortRange, TimeFormat, VecOrString, derive_deftly_template_Optionalify,
        serialization::SerializeAsString,
    },
};

use derive_deftly::Deftly;

/// Minimum bandwidth we will accept in either direction.
/// You have to have a limit somewhere; zero doesn't work. So I chose 1200 baud ...
pub(crate) const MINIMUM_BANDWIDTH: u64 = 150;

pub(crate) const MINIMUM_UDP_BUFFER: u64 = 1024; // ridiculously small, but you have to have a limit somewhere.

/// The set of configurable options supported by qcp.
///
/// **IMPORTANT:** The server and client configurations are combined at runtime.
/// See [`combine_bandwidth_configurations`](crate::transport::combine_bandwidth_configurations) for details.
///
/// ## tl;dr
///
/// For good performance, at the very least, configure:
/// * [rx](Configuration#structfield.rx); and
/// * [tx](Configuration#structfield.tx) if it is significantly different from rx.
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
#[derive(PartialEq, Deftly, FieldNamesAsSlice, Serialize)]
// note: the above derives are BEFORE Optionalify, so do NOT get passed on to the optionalified struct
#[derive_deftly(Optionalify)]
#[deftly(visibility = "pub(crate)")]
// note: the following derives are AFTER Optionalify, so DO get passed on to the optionalified struct
#[derive(Clone, Debug, Parser, Deserialize)]
pub struct Configuration {
    // TRANSPORT PARAMETERS ============================================================================
    // System bandwidth, UDP ports, timeout.
    /// The maximum network bandwidth we expect receiving data FROM the remote system.
    /// [default: 12.5M]
    ///
    /// <div class="warning">
    /// <big><b>This is the single most important configuration necessary for good performance!</b></big>
    ///
    /// If you configure nothing else, at least set this to suit your network.
    /// </div>
    ///
    /// This parameter is always interpreted as the **local** bandwidth, whether operating in client or server mode.
    ///
    /// This may be specified directly as a number, or as an SI quantity
    /// like `10M` or `256k`. **Note that this is described in BYTES, not bits**;
    /// if (for example) you expect to fill a 1Gbit ethernet connection,
    /// 125M would be a suitable setting.
    /// See also [`EngineeringQuantity`](https://docs.rs/engineering-repr/latest/engineering_repr/struct.EngineeringQuantity.html).
    ///
    #[arg(
        long,
        alias("rx-bw"),
        display_order(1),
        value_name = "bytes",
        help_heading("Network tuning"),
        long_help(r"
The maximum network bandwidth we expect receiving data FROM the remote system.
[default: 12.5M]

This is the single most important configuration necessary for good performance!
If you configure nothing else, at least set this to suit your network.

This parameter is always interpreted as the _local_ bandwidth, whether operating in client or server mode.

This may be specified directly as a number, or as an SI quantity
like `10M` or `256k`. Note that this is described in BYTES, not bits;
if (for example) you expect to fill a 1Gbit ethernet connection,
125M would be a suitable setting.
        "),
    )]
    pub rx: EngineeringQuantity<u64>,
    /// The maximum network bandwidth we expect sending data TO the remote system,
    /// if it is different from the bandwidth FROM the system.
    /// (For example, when you are connected via an asymmetric last-mile DSL or fibre profile.)
    ///
    /// Specify as a number, or as an SI quantity (e.g. `10M`).
    ///
    /// This parameter is always interpreted as the **local** bandwidth, whether operating in client or server mode.
    ///
    /// If not specified or 0, uses the value of `rx`.
    ///
    /// See also [`EngineeringQuantity`](https://docs.rs/engineering-repr/latest/engineering_repr/struct.EngineeringQuantity.html).
    #[arg(
        long,
        alias("tx-bw"),
        display_order(1),
        value_name = "bytes",
        help_heading("Network tuning"),
        long_help(r"
The maximum network bandwidth we expect sending data TO the remote system,
if it is different from the bandwidth FROM the system.
(For example, when you are connected via an asymmetric last-mile DSL or fibre profile.)

Specify as a number, or as an SI quantity (e.g. `10M`).

This parameter is always interpreted as the _local_ bandwidth, whether operating in client or server mode.
If not specified or 0, uses the value of `rx`.
"),
    )]
    pub tx: EngineeringQuantity<u64>,

    /// The expected network Round Trip time to the target system, in milliseconds.
    /// [default: 300]
    #[arg(
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
    /// _Setting this value too high reduces performance!_
    ///
    /// This may be specified directly as a number, or as an SI quantity like `10k`.
    ///
    /// See also [`EngineeringQuantity`](https://docs.rs/engineering-repr/latest/engineering_repr/struct.EngineeringQuantity.html).
    ///
    #[arg(
        long,
        value_name = "bytes",
        alias("cwnd"),
        display_order(0),
        help_heading("Advanced network tuning"),
        long_help(
            r"
(Network wizards only!)

The initial value for the sending congestion control window, in bytes.
If unspecified, the active congestion control algorithm decides.

Setting this value too high reduces performance!

This may be specified directly as a number, or as an SI quantity like `10k`."
        )
    )]
    pub initial_congestion_window: EngineeringQuantity<u64>,

    /// Uses the given UDP port or range on the **local** endpoint.
    /// This can be useful when there is a firewall between the endpoints.
    ///
    /// For example: `12345`, `20000-20100`
    ///
    /// If unspecified, uses any available UDP port.
    #[arg(long, value_name("M-N"), help_heading("Connection"), display_order(0))]
    pub port: PortRange,

    /// Connection timeout for the QUIC endpoints [seconds; default 5]
    ///
    /// This needs to be long enough for your network connection, but short enough to provide
    /// a timely indication that UDP may be blocked.
    #[arg(long, value_name("sec"), help_heading("Connection"), display_order(0))]
    pub timeout: u16,

    /// Size of the UDP kernel buffer in bytes.
    ///
    /// Specify as an integer or as an SI quantity, e.g. 4M.
    ///
    /// The default, 4M, should be good for most cases.
    /// However there may be high-bandwidth situations (10Gbps or more) where this becomes a bottleneck,
    /// or situations where you wish to restrict memory consumption.
    #[arg(long, help_heading("Advanced network tuning"), value_name = "bytes")]
    pub udp_buffer: EngineeringQuantity<u64>,

    /// Packet reordering loss detection threshold
    ///
    /// The default, 3, should be good for most cases.
    /// See RFC 9002 s6.1 for more details.
    #[arg(long, help_heading("Advanced network tuning"), value_name = "packets")]
    pub packet_threshold: u32,

    /// Time reordering loss detection threshold
    ///
    /// The default, 1.125, should be good for most cases.
    /// See RFC 9002 s6.1 for more details.
    #[arg(
        long,
        help_heading("Advanced network tuning"),
        value_name = "multiples of RTT"
    )]
    pub time_threshold: f32,

    /// The maximum UDP payload size to use before initial MTU discovery has completed (default: 1200)
    ///
    /// QUIC runs dynamic Path MTU detection, so this option is not necessary.
    ///
    /// Setting it appropriately can speed up the initial transfer phase, particularly if
    /// jumbo frames are in use.
    ///
    /// Setting it higher than supported will cause very poor performance while QUIC deals with blackhole
    /// events and figures out what the network is actually capable of.
    #[arg(long, help_heading("Advanced network tuning"), value_name = "bytes")]
    pub initial_mtu: u16,

    /// The minimum MTU that the network is guaranteed to support.
    ///
    /// Unless you have very good control over all the network infrastructure in use,
    /// this setting is unlikely to help you. The default, 1200, is the protocol minimum.
    ///
    /// Setting this higher than the network actually supports will cause very poor performance and unpredictable
    /// effects; it may not be possible to complete a file transfer in a reasonable time.
    #[arg(long, help_heading("Advanced network tuning"), value_name = "bytes")]
    pub min_mtu: u16,

    /// The maximum value that Path MTU discovery will search for (default: 1452)
    ///
    /// The maximum MTU only really affects the sending direction of the connection.
    ///
    /// If jumbo frames are possible with your end-to-end network connection, set this appropriately.
    ///
    /// The default is reasonably conservative. Depending on your network connection and any tunnelling
    /// or VPN in use, hosts connected by ethernet may be able to support a slightly higher maximum MTU.
    ///
    /// Some connections do not support even this MTU, so for best efficiency - particularly with small
    /// file transfers - it may be worth setting this lower to avoid the penalty caused by MTU detection
    /// triggering black hole behaviour.
    ///
    /// It is safe to set a high limit, but that may reduce efficiency as MTU discovery will take longer
    /// to complete.
    #[arg(long, help_heading("Advanced network tuning"), value_name = "bytes")]
    pub max_mtu: u16,

    // CLIENT OPTIONS ==================================================================================
    /// Forces use of a particular IP version when connecting to the remote. [default: any]
    ///
    // (see also [CliArgs::ipv4_alias__] and [CliArgs::ipv6_alias__])
    #[arg(
        long,
        help_heading("Connection"),
        group("ip address"),
        value_name("family"),
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
        display_order(0),
        help_heading("Connection"),
        long_help(
            r"
Provides an additional option or argument to pass to the ssh client. [default: none]

On the command line, you must repeat `-S` for each argument.
For example, to pass `-i /dev/null` to ssh, specify: `-S -i -S /dev/null`"
        )
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
    #[arg(long, value_name("FILE"), help_heading("Connection"), display_order(0))]
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
        value_name("mode"),
        long_help(r"Colour mode for console output (default: auto)

Passing `--color` without a value is equivalent to `--color always`.

Note that color configuration is not shared with the remote system, so the color output
from the remote system (log messages, remote-config) will be coloured per the
config file on the remote system.

qcp also supports the `CLICOLOR`, `CLICOLOR_FORCE` and `NO_COLOR` environment variables.
See https://bixense.com/clicolors/ for more details.

CLI options take precedence over the configuration file, which takes precedence over environment variables."),
    )]
    pub color: ColourMode,

    // OTHER PARAMETERS ================================================================================
    /// Forces the use of a particular TLS authentication type
    /// (default: any)
    #[arg(long, value_name("type"), help_heading("Connection"), display_order(0),
        value_parser(clap::builder::EnumValueParser::<CredentialsType>::new().map(SerializeAsString)),
    )]
    pub tls_auth_type: SerializeAsString<CredentialsType>,

    /// Use only AES256 cipher suites as far as possible
    ///
    /// This option is included for those who have a preference to use only AES256-based
    /// algorithms (aka CNSA 2.0). Note this option does not fully disable AES128, which is required for
    /// the QUIC 1.0 Initial Packet.
    #[arg(
        long,
        default_missing_value("true"), // required for a bool in Configuration, along with num_args and require_equals
        num_args(0..=1),
        require_equals(true),
        help_heading("Connection"),
    )]
    pub aes256: bool,
}

static SYSTEM_DEFAULT_CONFIG: LazyLock<Configuration> = LazyLock::new(|| Configuration {
    // Transport
    rx: 12_500_000u64.into(), // 100Mbit
    tx: 0u64.into(),
    rtt: 300,
    congestion: CongestionController::Cubic.into(),
    initial_congestion_window: 0u64.into(),
    port: PortRange::default(),
    timeout: 5,
    // https://fasterdata.es.net/host-tuning/linux/udp-tuning/ recommends 4M as good for most settings
    udp_buffer: 4_000_000u64.into(),
    packet_threshold: 3,     // default from Quinn
    time_threshold: 9. / 8., // default from Quinn
    initial_mtu: 1200,       // same as Quinn
    min_mtu: 1200,           // same as Quinn
    max_mtu: 1452,           // same as Quinn

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

    // Other
    tls_auth_type: CredentialsType::Any.into(),
    aes256: false,
});

impl Configuration {
    /// Computes the theoretical round-trip bandwidth-delay product for outbound data
    #[must_use]
    pub(crate) fn rtt_bandwidth_delay_product_tx(&self) -> u64 {
        self.tx() * u64::from(self.rtt) / 1000
    }
    /// Computes the theoretical round-trip bandwidth-delay product for inbound data
    #[must_use]
    pub(crate) fn rtt_bandwidth_delay_product_rx(&self) -> u64 {
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

    /// QUIC receive window
    #[must_use]
    pub fn recv_window(&self) -> u64 {
        // The theoretical in-flight limit appears to be sufficient
        self.rtt_bandwidth_delay_product_rx()
    }

    /// QUIC send window
    #[must_use]
    pub fn send_window(&self) -> u64 {
        // Quinn defaults to 8x the receive window
        8 * self.rtt_bandwidth_delay_product_tx()
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
            concat!(
                "rx {rx} ({rxbits}), tx {tx} ({txbits}), rtt {rtt}; ",
                "congestion algorithm {congestion} with initial window {iwind}; ",
                "send window {swnd}, receive window {rwnd}, ",
                "UDP buffer size {udp}; ",
                "packet_threshold {pkt_t}, time_threshold {tim_t}xRTT"
            ),
            tx = tx.human_count_bytes(),
            txbits = (tx * 8).human_count("bit"),
            rx = rx.human_count_bytes(),
            rxbits = (rx * 8).human_count("bit"),
            rtt = self.rtt_duration().human_duration(),
            congestion = self.congestion,
            iwind = iwind,
            swnd = self.send_window().human_count_bytes(),
            rwnd = self.recv_window().human_count_bytes(),
            udp = u64::from(self.udp_buffer).human_count_bytes(),
            pkt_t = self.packet_threshold,
            tim_t = self.time_threshold,
        )
    }

    /// Returns the system default settings
    #[must_use]
    pub fn system_default() -> &'static Self {
        &SYSTEM_DEFAULT_CONFIG
    }
}

// VALIDATION ------------------------------------------------------------

/// Data required by [`Configuration::try_validate()`]
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ValidationData {
    rtt: u16,
    rx: u64,
    tx: u64,
    udp: u64,
}

impl Configuration {
    fn validation_data(&self) -> ValidationData {
        ValidationData {
            rtt: self.rtt,
            rx: self.rx(),
            tx: self.tx(),
            udp: self.udp_buffer.into(),
        }
    }

    /// Performs additional validation checks on a configuration object
    pub(crate) fn try_validate(&self) -> Result<()> {
        let data = self.validation_data();
        let rtt = data.rtt;
        let rx = data.rx;
        #[allow(non_snake_case)] // look, it's a const
        let INFO = info();
        #[allow(non_snake_case)] // look, it's a const
        let RESET = reset();

        anyhow::ensure!(
            rx >= MINIMUM_BANDWIDTH,
            "The receive bandwidth ({INFO}rx {val}{RESET}B) is too small; it must be at least {min}",
            val = rx.to_eng(0),
            min = MINIMUM_BANDWIDTH.to_eng(3),
        );
        anyhow::ensure!(rtt > 0, "RTT cannot be zero");
        anyhow::ensure!(
            rx.checked_mul(rtt.into()).is_some(),
            "The receive bandwidth delay product calculation ({INFO}rx {val}{RESET}B x {INFO}rtt {rtt}{RESET}ms) overflowed",
            val = rx.to_eng(0),
        );

        let tx = data.tx;
        anyhow::ensure!(
            tx == 0 || tx >= MINIMUM_BANDWIDTH,
            "The transmit bandwidth ({INFO}tx {val}{RESET}B) is too small; it must be at least {min}",
            val = tx.to_eng(0),
            min = MINIMUM_BANDWIDTH.to_eng(3),
        );
        anyhow::ensure!(
            tx == 0 || tx.checked_mul(rtt.into()).is_some(),
            "The transmit bandwidth delay product calculation ({INFO}tx {val}{RESET}B x {INFO}rtt {rtt}{RESET}ms) overflowed",
            val = tx.to_eng(0),
        );

        let udp = data.udp;
        anyhow::ensure!(
            udp >= MINIMUM_UDP_BUFFER,
            "The UDP buffer size ({INFO}{udp}{RESET}) is too small; it must be at least {MINIMUM_UDP_BUFFER}",
        );

        anyhow::ensure!(
            self.min_mtu >= 1200,
            "Minimum MTU ({mtu}) cannot be less than 1200",
            mtu = self.min_mtu
        );
        anyhow::ensure!(
            self.max_mtu >= 1200,
            "Maximum MTU ({mtu}) cannot be less than 1200",
            mtu = self.max_mtu
        );
        anyhow::ensure!(
            self.initial_mtu >= 1200,
            "Initial MTU ({mtu}) cannot be less than 1200",
            mtu = self.initial_mtu
        );
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
    use crate::{
        Configuration,
        config::{ColourMode, Manager},
        protocol::control::{CongestionController, CredentialsType},
        util::{AddressFamily, TimeFormat},
    };

    use super::SYSTEM_DEFAULT_CONFIG;
    use assertables::assert_contains;
    use littertray::LitterTray;
    use pretty_assertions::assert_eq;

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
        fn tc<T: Fn(&mut crate::Configuration)>(func: T, expected: &str, expected2: Option<&str>) {
            let mut cfg = SYSTEM_DEFAULT_CONFIG.clone();
            func(&mut cfg);
            let str = cfg.try_validate().unwrap_err().to_string();
            let error = console::strip_ansi_codes(&str);
            assert_contains!(error, expected);
            if let Some(expected2_) = expected2 {
                assert_contains!(error, expected2_);
            }
        }

        let cfg = SYSTEM_DEFAULT_CONFIG.clone();
        assert!(cfg.try_validate().is_ok());

        tc(
            |c| c.rx = 1u64.into(),
            "receive bandwidth (rx 1B) is too small",
            None,
        );
        tc(
            |c| c.tx = 1u64.into(),
            "transmit bandwidth (tx 1B) is too small",
            None,
        );
        tc(
            |c| c.rx = u64::MAX.into(),
            "receive bandwidth delay product calculation",
            Some("overflowed"),
        );
        tc(
            |c| c.tx = u64::MAX.into(),
            "transmit bandwidth delay product calculation",
            Some("overflowed"),
        );
        tc(|c| c.rtt = 0, "RTT cannot be zero", None);
        tc(
            |c| c.udp_buffer = 0u64.into(),
            "The UDP buffer size (0) is too small",
            None,
        );
        tc(|c| c.min_mtu = 0, "Minimum MTU (0) cannot be ", None);
        tc(|c| c.max_mtu = 0, "Maximum MTU (0) cannot be ", None);
        tc(|c| c.initial_mtu = 0, "Initial MTU (0) cannot be ", None);
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

    fn assert_cfg_parseable(data: &str) -> Configuration {
        use crate::config::{Configuration, Configuration_Optional};
        let mut mgr = LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(path, data);
            let mut mgr = Manager::without_files(None);
            mgr.merge_ssh_config(path, None, false);
            Ok(mgr)
        })
        .unwrap();
        let cfg_opt = mgr.get::<Configuration_Optional>();
        assert!(cfg_opt.is_ok(), "optional config failed for case {data}");
        mgr.apply_system_default();
        let cfg = mgr.get::<Configuration>();
        assert!(cfg.is_ok(), "non-optional config failed for case {data}");
        cfg.unwrap()
    }

    #[test]
    #[should_panic(expected = "optional config failed")]
    #[allow(unused_results)]
    fn cfg_unparseable_enum() {
        assert_cfg_parseable("congestion nosuchalgorithm");
    }

    #[test]
    fn cfg_ssh_options_regression() {
        let c = assert_cfg_parseable("sshoptions");
        assert_eq!(c.ssh_options, vec![].into());
        let c = assert_cfg_parseable("sshoptions a");
        assert_eq!(c.ssh_options, vec!["a".into()].into());
        let c = assert_cfg_parseable("sshoptions a b");
        assert_eq!(c.ssh_options, vec!["a".into(), "b".into()].into());
    }
}
