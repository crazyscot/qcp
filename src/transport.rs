//! QUIC transport configuration
// (c) 2024 Ross Younger

use std::{str::FromStr, sync::Arc, time::Duration};

use anyhow::Result;
use human_repr::HumanCount as _;
use quinn::{
    congestion::{BbrConfig, CubicConfig},
    TransportConfig,
};
use serde::{de, Deserialize, Serialize};
use strum::VariantNames;
use tracing::debug;

use crate::config::Configuration;

/// Keepalive interval for the QUIC connection
pub const PROTOCOL_KEEPALIVE: Duration = Duration::from_secs(5);

/// Specifies whether to configure to maximise transmission throughput, receive throughput, or both.
/// Specifying `Both` for a one-way data transfer will work, but wastes kernel memory.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ThroughputMode {
    /// We expect to send a lot but not receive
    Tx,
    /// We expect to receive a lot but not send much
    Rx,
    /// We expect to send and receive, or we don't know
    Both,
}

/// Selects the congestion control algorithm to use.
/// On the wire, this is serialized as a standard BARE enum.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    strum::Display,
    strum::EnumString,
    strum::FromRepr,
    strum::VariantNames,
    clap::ValueEnum,
    Serialize,
)]
#[strum(serialize_all = "lowercase")] // N.B. this applies to EnumString, not Display
#[repr(u8)]
pub enum CongestionControllerType {
    /// The congestion algorithm TCP uses. This is good for most cases.
    #[default]
    Cubic = 0,
    /// (Use with caution!) An experimental algorithm created by Google,
    /// which increases goodput in some situations
    /// (particularly long and fat connections where the intervening
    /// buffers are shallow). However this comes at the cost of having
    /// more data in-flight, and much greater packet retransmission.
    /// See
    /// `https://blog.apnic.net/2020/01/10/when-to-use-and-not-use-bbr/`
    /// for more discussion.
    Bbr = 1,
}

impl<'de> Deserialize<'de> for CongestionControllerType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let lower = s.to_ascii_lowercase();
        // requires strum::EnumString && strum::VariantNames && #[strum(serialize_all = "lowercase")]
        FromStr::from_str(&lower)
            .map_err(|_| de::Error::unknown_variant(&s, CongestionControllerType::VARIANTS))
    }
}

/// Creates a `quinn::TransportConfig` for the endpoint setup
pub fn create_config(params: &Configuration, mode: ThroughputMode) -> Result<Arc<TransportConfig>> {
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
                .datagram_send_buffer_size(Configuration::send_buffer().try_into()?);
        }
        ThroughputMode::Rx => (),
    }
    #[allow(clippy::cast_possible_truncation)]
    match mode {
        // TODO: If we later support multiple streams at once, will need to consider receive_window and stream_receive_window.
        ThroughputMode::Rx | ThroughputMode::Both => {
            let _ = config
                .stream_receive_window(params.recv_window().try_into()?)
                .datagram_receive_buffer_size(Some(Configuration::recv_buffer() as usize));
        }
        ThroughputMode::Tx => (),
    }

    let window = params.initial_congestion_window;
    match params.congestion {
        CongestionControllerType::Cubic => {
            let mut cubic = CubicConfig::default();
            if window != 0 {
                let _ = cubic.initial_window(window);
            }
            let _ = config.congestion_controller_factory(Arc::new(cubic));
        }
        CongestionControllerType::Bbr => {
            let mut bbr = BbrConfig::default();
            if window != 0 {
                let _ = bbr.initial_window(window);
            }
            let _ = config.congestion_controller_factory(Arc::new(bbr));
        }
    }

    debug!(
        "Network configuration: {}",
        params.format_transport_config()
    );
    debug!(
        "Buffer configuration: send window {sw}, buffer {sb}; recv window {rw}, buffer {rb}",
        sw = params.send_window().human_count_bytes(),
        sb = Configuration::send_buffer().human_count_bytes(),
        rw = params.recv_window().human_count_bytes(),
        rb = Configuration::recv_buffer().human_count_bytes()
    );

    Ok(config.into())
}
