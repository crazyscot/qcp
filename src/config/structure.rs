//! Configuration structure
// (c) 2024 Ross Younger

use std::time::Duration;

use clap::Parser;
use human_repr::{HumanCount as _, HumanDuration as _};
use serde::{Deserialize, Serialize};

use crate::{
    protocol::control::ConnectionType,
    transport::CongestionControllerType,
    util::{derive_deftly_template_Optionalify, humanu64::HumanU64, PortRange},
};
use derive_deftly::Deftly;

/// The set of configurable options supported by qcp.
///
/// **Note:** On this struct, `default()` returns qcp's hard-wired configuration defaults.
#[derive(Deftly)]
#[derive_deftly(Optionalify)]
#[deftly(visibility = "pub(crate)")]
#[derive(Debug, Clone, PartialEq, Eq, Parser, Deserialize, Serialize)]
pub struct Configuration {
    // TRANSPORT PARAMETERS ============================================================================
    // System bandwidth, UDP ports, timeout.
    /// The maximum network bandwidth we expect receiving data FROM the remote system.
    /// [default: 12500k]
    ///
    /// This may be specified directly as a number of bytes, or as an SI quantity
    /// e.g. "10M" or "256k". Note that this is described in BYTES, not bits;
    /// if (for example) you expect to fill a 1Gbit ethernet connection,
    /// 125M might be a suitable setting.
    #[arg(short('b'), long, alias("rx-bw"), help_heading("Network tuning"), display_order(10), value_name="bytes", value_parser=clap::value_parser!(HumanU64))]
    pub rx: HumanU64,
    /// The maximum network bandwidth we expect sending data TO the remote system,
    /// if it is different from the bandwidth FROM the system.
    /// [default: use the value of --rx]
    ///
    /// (For example, when you are connected via an asymmetric last-mile DSL or fibre profile.)
    #[arg(short('B'), long, alias("tx-bw"), help_heading("Network tuning"), display_order(10), value_name="bytes", value_parser=clap::value_parser!(HumanU64))]
    pub tx: Option<HumanU64>,

    /// The expected network Round Trip time to the target system, in milliseconds.
    /// [default: 300]
    #[arg(
        short('r'),
        long,
        help_heading("Network tuning"),
        display_order(1),
        value_name("ms")
    )]
    pub rtt: u16,

    /// Specifies the congestion control algorithm to use.
    /// [default: cubic]
    #[arg(
        long,
        action,
        value_name = "alg",
        help_heading("Advanced network tuning")
    )]
    #[clap(value_enum)]
    pub congestion: CongestionControllerType,

    /// (Network wizards only!)
    /// The initial value for the sending congestion control window.
    /// [default: use the default for the selected congestion control algorithm]
    ///
    /// Setting this value too high reduces performance!
    #[arg(long, help_heading("Advanced network tuning"), value_name = "bytes")]
    pub initial_congestion_window: Option<u64>,

    /// Uses the given UDP port or range on the local endpoint.
    ///
    /// This can be useful when there is a firewall between the endpoints.
    #[arg(short = 'p', long, value_name("M-N"), help_heading("Connection"))]
    pub port: Option<PortRange>,

    /// Connection timeout for the QUIC endpoints [seconds; default 5]
    ///
    /// This needs to be long enough for your network connection, but short enough to provide
    /// a timely indication that UDP may be blocked.
    #[arg(short, long, value_name("sec"), help_heading("Connection"))]
    pub timeout: u16,

    // CLIENT OPTIONS ==================================================================================
    // These options are only used at the client end.
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

impl Configuration {
    pub(crate) fn address_family(&self) -> Option<ConnectionType> {
        if self.ipv4 {
            Some(ConnectionType::Ipv4)
        } else if self.ipv6 {
            Some(ConnectionType::Ipv6)
        } else {
            None
        }
    }

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
        *self.rx
    }
    #[must_use]
    /// Transmit bandwidth (accessor)
    pub fn tx(&self) -> u64 {
        if let Some(tx) = self.tx {
            *tx
        } else {
            self.rx()
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

    /// Converts the `timeout` field into a Duration
    #[must_use]
    pub fn timeout_duration(&self) -> Duration {
        Duration::from_secs(self.timeout.into())
    }

    /// Formats the transport-related options for display
    #[must_use]
    pub fn format_transport_config(&self) -> String {
        let iwind = match self.initial_congestion_window {
            None => "<default>".to_string(),
            Some(s) => s.human_count_bytes().to_string(),
        };
        let (tx, rx) = (self.tx(), self.rx());
        format!(
            "rx {rx} ({rxbits}), tx {tx} ({txbits}), rtt {rtt}, congestion algorithm {congestion:?} with initial window {iwind}",
            tx = tx.human_count_bytes(),
            txbits = (tx * 8).human_count("bit"),
            rx = rx.human_count_bytes(),
            rxbits = (rx * 8).human_count("bit"),
            rtt = self.rtt_duration().human_duration(),
            congestion = self.congestion,
        )
    }
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            // Transport
            rx: 12_500_000.into(),
            tx: None,
            rtt: 300,
            congestion: CongestionControllerType::Cubic,
            initial_congestion_window: None,
            port: None,
            timeout: 5,

            // Client
            ipv4: false,
            ipv6: false,
            ssh: "ssh".into(),
            ssh_opt: vec![],
            remote_port: None,
        }
    }
}

#[cfg(test)]
mod test {
    use super::Configuration;

    #[test]
    fn flattened() {
        let v = Configuration::default();
        let j = serde_json::to_string(&v).unwrap();
        let d = json::parse(&j).unwrap();
        assert!(!d.has_key("bw"));
        assert!(d.has_key("rtt"));
    }
}
