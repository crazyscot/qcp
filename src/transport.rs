//! QUIC transport configuration
// (c) 2024 Ross Younger

use std::{fmt::Display, sync::Arc, time::Duration};

use anyhow::Result;
use clap::Parser;
use human_repr::{HumanCount, HumanDuration as _};
use humanize_rs::bytes::Bytes;
use quinn::{
    congestion::{BbrConfig, CubicConfig},
    TransportConfig,
};
use tracing::debug;

use crate::util::{parse_duration, PortRange};

/// Keepalive interval for the QUIC connection
pub const PROTOCOL_KEEPALIVE: Duration = Duration::from_secs(5);

/// Shared parameters used to set up the QUIC UDP connection
#[derive(Copy, Clone, Debug, Parser)]
pub struct QuicParams {
    /// Uses the given UDP port or range on the local endpoint.
    ///
    /// This can be useful when there is a firewall between the endpoints.
    #[arg(short = 'p', long, value_name("M-N"), help_heading("Connection"))]
    pub port: Option<PortRange>,

    /// Connection timeout for the QUIC endpoints.
    ///
    /// This needs to be long enough for your network connection, but short enough to provide
    /// a timely indication that UDP may be blocked.
    #[arg(short, long, default_value("5"), value_name("sec"), value_parser=parse_duration, help_heading("Connection"))]
    pub timeout: Duration,
}

/// Specifies whether to configure to maximise transmission throughput, receive throughput, or both.
/// Specifying `Both` for a one-way data transfer will work, but wastes kernel memory.
#[derive(Copy, Clone, Debug)]
pub enum ThroughputMode {
    /// We expect to send a lot but not receive
    Tx,
    /// We expect to receive a lot but not send much
    Rx,
    /// We expect to send and receive, or we don't know
    Both,
}

/// Selects the congestion control algorithm to use
#[derive(Copy, Clone, Debug, strum_macros::Display, clap::ValueEnum)]
#[strum(serialize_all = "kebab_case")]
pub enum CongestionControllerType {
    /// The congestion algorithm TCP uses. This is good for most cases.
    Cubic,
    /// (Use with caution!) An experimental algorithm created by Google,
    /// which increases goodput in some situations
    /// (particularly long and fat connections where the intervening
    /// buffers are shallow). However this comes at the cost of having
    /// more data in-flight, and much greater packet retransmission.
    /// See
    /// `https://blog.apnic.net/2020/01/10/when-to-use-and-not-use-bbr/`
    /// for more discussion.
    Bbr,
}

/// Parameters needed to set up transport configuration
#[derive(Copy, Clone, Debug, Parser)]
pub struct BandwidthParams {
    /// The maximum network bandwidth we expect receiving data FROM the remote system.
    ///
    /// This may be specified directly as a number of bytes, or as an SI quantity
    /// e.g. "10M" or "256k". Note that this is described in BYTES, not bits;
    /// if (for example) you expect to fill a 1Gbit ethernet connection,
    /// 125M might be a suitable setting.
    #[arg(short('b'), long, help_heading("Network tuning"), display_order(10), default_value("12500k"), value_name="bytes", value_parser=clap::value_parser!(Bytes<u64>))]
    pub rx_bw: Bytes<u64>,

    /// The maximum network bandwidth we expect sending data TO the remote system,
    /// if it is different from the bandwidth FROM the system.
    /// (For example, when you are connected via an asymmetric last-mile DSL or fibre profile.)
    /// [default: use the value of --rx-bw]
    #[arg(short('B'), long, help_heading("Network tuning"), display_order(10), value_name="bytes", value_parser=clap::value_parser!(Bytes<u64>))]
    pub tx_bw: Option<Bytes<u64>>,

    /// The expected network Round Trip time to the target system, in milliseconds.
    #[arg(
        short('r'),
        long,
        help_heading("Network tuning"),
        display_order(1),
        default_value("300"),
        value_name("ms")
    )]
    pub rtt: u16,

    /// Specifies the congestion control algorithm to use.
    #[arg(
        long,
        action,
        value_name = "alg",
        help_heading("Advanced network tuning")
    )]
    #[clap(value_enum, default_value_t=CongestionControllerType::Cubic)]
    pub congestion: CongestionControllerType,

    /// (Network wizards only!)
    /// The initial value for the sending congestion control window.
    ///
    /// Setting this value too high reduces performance!
    ///
    /// If not specified, this setting is determined by the selected
    /// congestion control algorithm.
    #[arg(long, help_heading("Advanced network tuning"), value_name = "bytes")]
    pub initial_congestion_window: Option<u64>,
}

impl Display for BandwidthParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let iwind = match self.initial_congestion_window {
            None => "<default>".to_string(),
            Some(s) => s.human_count_bytes().to_string(),
        };
        let (tx, rx) = (self.tx(), self.rx());
        write!(
            f,
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

impl BandwidthParams {
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
        self.rx_bw.size()
    }
    #[must_use]
    /// Transmit bandwidth (accessor)
    pub fn tx(&self) -> u64 {
        if let Some(tx) = self.tx_bw {
            tx.size()
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
    pub fn send_buffer(&self) -> u64 {
        // UDP kernel buffers of 2MB have proven sufficient to get close to line speed on a 300Mbit downlink with 300ms RTT.
        2_097_152
    }
    /// UDP kernel receive buffer size to use
    #[must_use]
    pub fn recv_buffer(&self) -> u64 {
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
}

/// Creates a `quinn::TransportConfig` for the endpoint setup
pub fn create_config(
    params: BandwidthParams,
    mode: ThroughputMode,
) -> Result<Arc<TransportConfig>> {
    let mut config = TransportConfig::default();
    let _ = config
        .max_concurrent_bidi_streams(1u8.into())
        .max_concurrent_uni_streams(0u8.into())
        .keep_alive_interval(Some(PROTOCOL_KEEPALIVE))
        .allow_spin(true);

    match mode {
        ThroughputMode::Tx | ThroughputMode::Both => {
            let _ = config
                .send_window(params.send_window())
                .datagram_send_buffer_size(params.send_buffer().try_into()?);
        }
        ThroughputMode::Rx => (),
    }
    #[allow(clippy::cast_possible_truncation)]
    match mode {
        // TODO: If we later support multiple streams at once, will need to consider receive_window and stream_receive_window.
        ThroughputMode::Rx | ThroughputMode::Both => {
            let _ = config
                .stream_receive_window(params.recv_window().try_into()?)
                .datagram_receive_buffer_size(Some(params.recv_buffer() as usize));
        }
        ThroughputMode::Tx => (),
    }

    match params.congestion {
        CongestionControllerType::Cubic => {
            let mut cubic = CubicConfig::default();
            if let Some(w) = params.initial_congestion_window {
                let _ = cubic.initial_window(w);
            }
            let _ = config.congestion_controller_factory(Arc::new(cubic));
        }
        CongestionControllerType::Bbr => {
            let mut bbr = BbrConfig::default();
            if let Some(w) = params.initial_congestion_window {
                let _ = bbr.initial_window(w);
            }
            let _ = config.congestion_controller_factory(Arc::new(bbr));
        }
    }

    debug!("Network configuration: {params}");
    debug!(
        "Buffer configuration: send window {sw}, buffer {sb}; recv window {rw}, buffer {rb}",
        sw = params.send_window().human_count_bytes(),
        sb = params.send_buffer().human_count_bytes(),
        rw = params.recv_window().human_count_bytes(),
        rb = params.recv_buffer().human_count_bytes()
    );

    Ok(config.into())
}
