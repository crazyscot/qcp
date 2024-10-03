// QUIC transport configuration
// (c) 2024 Ross Younger

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use quinn::{congestion::CubicConfig, TransportConfig};

pub const SEND_BUFFER_SIZE: usize = 1048576;

/// Computes the theoretical receive window for a given bandwidth/RTT configuration
pub fn receive_window_for(bandwidth_limit: u64, rtt_ms: u16) -> u32 {
    (bandwidth_limit * (rtt_ms as u64) / 1000) as u32
}

/// In some cases the theoretical receive window is less than the system default.
/// In such a case, don't suggest setting it smaller, that would be silly.
pub fn practical_receive_window_for(bandwidth_limit: u64, rtt_ms: u16) -> Result<u32> {
    use std::net::UdpSocket;
    let theoretical = receive_window_for(bandwidth_limit, rtt_ms);
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    let current = crate::os::os::get_recvbuf(&sock)? as u32;
    Ok(std::cmp::max(theoretical, current))
}

pub fn config_factory(
    bandwidth_limit: u64,
    rtt_ms: u16,
    initial_window: u64,
) -> Result<Arc<TransportConfig>> {
    let rtt = Duration::from_millis(rtt_ms as u64);
    let receive_window = practical_receive_window_for(bandwidth_limit, rtt_ms)?;

    let mut config = TransportConfig::default();
    let _ = config
        .max_concurrent_bidi_streams(1u8.into())
        .max_concurrent_uni_streams(0u8.into())
        .initial_rtt(rtt)
        .stream_receive_window(receive_window.into())
        .send_window((receive_window * 8).into())
        .datagram_receive_buffer_size(Some(receive_window as usize))
        .datagram_send_buffer_size(SEND_BUFFER_SIZE);

    let mut cubic = CubicConfig::default();
    let _ = cubic.initial_window(initial_window);
    let _ = config.congestion_controller_factory(Arc::new(cubic));

    Ok(config.into())
}
