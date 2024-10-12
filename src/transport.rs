// QUIC transport configuration
// (c) 2024 Ross Younger

use std::{fmt::Display, sync::Arc, time::Duration};

use anyhow::Result;
use human_repr::{HumanCount, HumanDuration as _};
use quinn::{
    congestion::{BbrConfig, CubicConfig},
    TransportConfig,
};
use tracing::debug;

use crate::cli::CliArgs;

/// Keepalive interval for the QUIC connection
pub const PROTOCOL_KEEPALIVE: Duration = Duration::from_secs(5);

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
#[derive(Copy, Clone, Debug)]
pub struct BandwidthParams {
    /// Max transmit bandwidth in bytes
    tx: u64,
    /// Max receive bandwidth in bytes
    rx: u64,
    /// Expected round trip time to the remote
    rtt: Duration,
    /// Initial congestion window (network wizards only!)
    initial_window: Option<u64>,
    /// Congestion controller selection
    congestion: CongestionControllerType,
}

impl Display for BandwidthParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let iwind = match self.initial_window {
            None => "<default>".to_string(),
            Some(s) => s.human_count_bytes().to_string(),
        };
        write!(
            f,
            "tx {tx} ({txbits}), rx {rx} ({rxbits}), rtt {rtt}, congestion algorithm {congestion:?} with initial window {iwind}",
            tx = self.tx.human_count_bytes(),
            txbits = (self.tx * 8).human_count("bit"),
            rx = self.rx.human_count_bytes(),
            rxbits = (self.rx * 8).human_count("bit"),
            rtt = self.rtt.human_duration(),
            congestion = self.congestion,
        )
    }
}

impl From<&CliArgs> for BandwidthParams {
    fn from(args: &CliArgs) -> Self {
        Self {
            rx: args.rx_bw.size(),
            tx: args.tx_bw.unwrap_or(args.rx_bw).size(),
            rtt: Duration::from_millis(u64::from(args.rtt)),
            initial_window: args.initial_congestion_window,
            congestion: args.congestion,
        }
    }
}

impl BandwidthParams {
    /// Computes the theoretical bandwidth-delay product for outbound data
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn bandwidth_delay_product_tx(&self) -> u64 {
        self.tx * self.rtt.as_millis() as u64 / 1000
    }
    /// Computes the theoretical bandwidth-delay product for inbound data
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn bandwidth_delay_product_rx(&self) -> u64 {
        self.rx * self.rtt.as_millis() as u64 / 1000
    }
    #[must_use]
    /// Receive bandwidth (accessor)
    pub fn rx(&self) -> u64 {
        self.rx
    }
    #[must_use]
    /// Transmit bandwidth (accessor)
    pub fn tx(&self) -> u64 {
        self.tx
    }
}

#[derive(Debug, Copy, Clone)]
/// Computed buffer configuration
pub(crate) struct BandwidthConfig {
    pub(crate) send_window: u64,
    pub(crate) send_buffer: u64,
    pub(crate) recv_window: u64,
    pub(crate) recv_buffer: u64,
}

impl Display for BandwidthConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "send window {sw}, buffer {sb}; recv window {rw}, buffer {rb}",
            sw = self.send_window.human_count_bytes(),
            sb = self.send_buffer.human_count_bytes(),
            rw = self.recv_window.human_count_bytes(),
            rb = self.recv_buffer.human_count_bytes()
        )
    }
}

impl From<BandwidthParams> for BandwidthConfig {
    fn from(params: BandwidthParams) -> Self {
        Self::from(&params)
    }
}

impl From<&BandwidthParams> for BandwidthConfig {
    fn from(params: &BandwidthParams) -> Self {
        // Start with the BDP, which is the theoretical in flight limit
        let bdp_rx = params.bandwidth_delay_product_rx();
        let bdp_tx = params.bandwidth_delay_product_tx();

        // However there might be random added latency en route, so provide for a larger send window than theoretical.
        Self {
            send_window: 2 * bdp_tx,
            recv_window: bdp_rx,
            // UDP kernel buffers of 2MB have proven sufficient to get close to line speed on a 300Mbit downlink with 300ms RTT.
            send_buffer: 2_097_152,
            recv_buffer: 2_097_152,
        }
    }
}

/// Creates a `quinn::TransportConfig` for the endpoint setup
pub fn create_config(
    params: BandwidthParams,
    mode: ThroughputMode,
) -> Result<Arc<TransportConfig>> {
    let congestion = params.congestion;
    let bcfg: BandwidthConfig = params.into();

    // Common setup
    let mut config = TransportConfig::default();
    let _ = config
        .max_concurrent_bidi_streams(1u8.into())
        .max_concurrent_uni_streams(0u8.into())
        .keep_alive_interval(Some(PROTOCOL_KEEPALIVE))
        .allow_spin(true);

    match mode {
        ThroughputMode::Tx | ThroughputMode::Both => {
            let _ = config
                .send_window(bcfg.send_window)
                .datagram_send_buffer_size(bcfg.send_buffer.try_into()?);
        }
        ThroughputMode::Rx => (),
    }
    #[allow(clippy::cast_possible_truncation)]
    match mode {
        // TODO: If we later support multiple streams at once, will need to consider receive_window and stream_receive_window.
        ThroughputMode::Rx | ThroughputMode::Both => {
            let _ = config
                .stream_receive_window(bcfg.recv_window.try_into()?)
                .datagram_receive_buffer_size(Some(bcfg.recv_buffer as usize));
        }
        ThroughputMode::Tx => (),
    }

    match congestion {
        CongestionControllerType::Cubic => {
            let mut cubic = CubicConfig::default();
            if let Some(w) = params.initial_window {
                let _ = cubic.initial_window(w);
            }
            let _ = config.congestion_controller_factory(Arc::new(cubic));
        }
        CongestionControllerType::Bbr => {
            let mut bbr = BbrConfig::default();
            if let Some(w) = params.initial_window {
                let _ = bbr.initial_window(w);
            }
            let _ = config.congestion_controller_factory(Arc::new(bbr));
        }
    }

    debug!("Network configuration: {params}");
    debug!("Buffer configuration: {bcfg}",);

    Ok(config.into())
}
