//! Statistics processing and output
// (c) 2024 Ross Younger

use engineering_repr::EngineeringRepr as _;
use human_repr::{HumanCount, HumanDuration, HumanThroughput};
use num_format::ToFormattedString as _;
use quinn::ConnectionStats;
use std::{
    cmp::{self, max},
    fmt::Display,
    time::Duration,
};
use tracing::{info, warn};

use crate::{
    config::Configuration,
    protocol::{
        Variant,
        control::{ClosedownReportExtension, ClosedownReportV1, Direction},
    },
    session::CommandStats,
};

/// Human friendly output helper
#[derive(Debug, Clone, Copy)]
pub(crate) struct DataRate {
    /// Bytes per second; if None, we were unable to compute a rate.
    rate: Option<f64>,
}

impl DataRate {
    /// Standard constructor
    #[must_use]
    pub(crate) fn new(bytes: u64, time: Option<Duration>) -> Self {
        match time {
            None => Self { rate: None },
            Some(time) if time.is_zero() => Self { rate: None }, // divide by zero is not meaningful
            Some(time) => Self {
                #[allow(clippy::cast_precision_loss)]
                rate: Some((bytes as f64) / time.as_secs_f64()),
            },
        }
    }
    /// Accessor
    #[must_use]
    pub(crate) fn byte_rate(&self) -> Option<f64> {
        self.rate
    }
}

impl Display for DataRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.byte_rate() {
            None => f.write_str("unknown"),
            Some(rate) => rate.human_throughput_bytes().fmt(f),
        }
    }
}

/// Output the end-of-game statistics
#[cfg_attr(coverage_nightly, coverage(off))]
// this is a cosmetic function, it is not practical to test in its current form
pub(crate) fn process_statistics(
    stats: &ConnectionStats,
    command_stats: CommandStats,
    transport_time: Option<Duration>,
    remote_stats: &ClosedownReportV1,
    bandwidth: &Configuration,
    show_statistics: bool,
    direction: Direction,
) {
    #![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

    let locale = &num_format::Locale::en;
    let payload_bytes = command_stats.payload_bytes;
    if payload_bytes != 0 {
        let size = payload_bytes.human_count_bytes();
        let rate = crate::util::stats::DataRate::new(payload_bytes, transport_time);
        let transport_time_str =
            transport_time.map_or("unknown".to_string(), |d| d.human_duration().to_string());

        let peak_rate = max(
            command_stats.peak_transfer_rate,
            rate.byte_rate().unwrap_or_default() as u64,
        );
        let peak = peak_rate.human_throughput_bytes();
        info!("Transferred {size} in {transport_time_str}; average {rate}; peak {peak}");
    }
    if show_statistics {
        info!(
            "Total packets sent: {} by us; {} by remote",
            stats.path.sent_packets.to_formatted_string(locale),
            remote_stats.sent_packets.0.to_formatted_string(locale),
        );
    }
    let congestion = stats.path.congestion_events + remote_stats.congestion_events.0;
    if congestion > 0 {
        warn!(
            "Congestion events detected: {}",
            congestion.human_count_bare()
        );
    }
    if stats.path.lost_packets > 0 {
        #[allow(clippy::cast_precision_loss)]
        let pct = 100. * stats.path.lost_packets as f64 / stats.path.sent_packets as f64;
        warn!(
            "Lost packets: {count}/{total} ({pct:.2}%, for {bytes})",
            count = stats.path.lost_packets.human_count_bare(),
            total = stats.path.sent_packets.human_count_bare(),
            bytes = stats.path.lost_bytes.human_count_bytes(),
        );
    }
    if remote_stats.lost_packets.0 > 0 {
        #[allow(clippy::cast_precision_loss)]
        let pct = 100. * remote_stats.lost_packets.0 as f64 / remote_stats.sent_packets.0 as f64;
        warn!(
            "Remote lost packets: {count}/{total} ({pct:.2}%, for {bytes})",
            count = remote_stats.lost_packets.0.human_count_bare(),
            total = remote_stats.sent_packets.0.human_count_bare(),
            bytes = remote_stats.lost_bytes.0.human_count_bytes(),
        );
    }
    if show_statistics {
        advanced_statistics(stats, command_stats, remote_stats);
    }
    check_rtt(stats, bandwidth);
    suggest_bandwidth_tuning(
        stats,
        command_stats,
        transport_time,
        remote_stats,
        bandwidth,
        direction,
    );
}

fn advanced_statistics(
    stats: &ConnectionStats,
    command_stats: CommandStats,
    remote_stats: &ClosedownReportV1,
) {
    let sender_sent_bytes = cmp::max(stats.udp_tx.bytes, remote_stats.sent_bytes.0);
    let locale = &num_format::Locale::en;
    let payload_bytes = command_stats.payload_bytes;

    {
        use crate::protocol::FindTag as _;
        use serde_bare::Uint;

        let cwnd = cmp::max(stats.path.cwnd, remote_stats.cwnd.0);
        let not_reported = Variant::from("<not reported>");

        let remote_pmtu: &Variant = remote_stats
            .extension
            .find_tag(ClosedownReportExtension::Pmtu)
            .unwrap_or(&not_reported);

        let remote_rtt = remote_stats
            .extension
            .find_tag(ClosedownReportExtension::Rtt);
        // RTT is microseconds
        let remote_rtt = if let Some(Variant::Unsigned(Uint(v))) = remote_rtt {
            Duration::from_micros(*v).human_duration().to_string()
        } else {
            "<not reported>".into()
        };
        info!(
            "Path MTU {pmtu} (remote: {remote_pmtu}), round-trip time {rtt} (remote: {remote_rtt}), final congestion window {cwnd}",
            pmtu = stats.path.current_mtu,
            rtt = stats.path.rtt.human_duration(),
            cwnd = cwnd.to_formatted_string(locale),
        );
        let black_holes = stats.path.black_holes_detected + remote_stats.black_holes.0;
        info!(
            "{tx} datagrams sent, {rx} received, {black_holes} black holes detected",
            tx = stats.udp_tx.datagrams.human_count_bare(),
            rx = stats.udp_rx.datagrams.human_count_bare(),
            black_holes = black_holes.to_formatted_string(locale),
        );
        if payload_bytes != 0 {
            #[allow(clippy::cast_precision_loss)]
            let overhead_pct =
                100. * (sender_sent_bytes - payload_bytes) as f64 / payload_bytes as f64;
            info!(
                "{} total bytes sent for {} bytes payload  ({:.2}% overhead/loss)",
                sender_sent_bytes.to_formatted_string(locale),
                payload_bytes.to_formatted_string(locale),
                overhead_pct
            );
        }
    }
}

fn check_rtt(stats: &ConnectionStats, bandwidth: &Configuration) {
    // Warn when RTT is 10% worse than the configuration.
    // No, seriously, nobody is going to have an RTT exceeding 2^64 ms. Even 2^32 (~49 days) is beyond unlikely.
    // This calculation overflows at RTT (2^32 / 100) ms, or about 11.9 hours.
    // Therefore it will likely go wrong in interstellar use, but that's not in scope right now.
    #[allow(clippy::cast_possible_truncation)]
    if (stats.path.rtt.as_millis() as u32) > u32::from(bandwidth.rtt) * 110 / 100 {
        warn!(
            "Measured path RTT {rtt_measured:?} was greater than configuration {rtt_arg}; for better performance, next time try --rtt {rtt_param}",
            rtt_measured = stats.path.rtt,
            rtt_arg = bandwidth.rtt,
            rtt_param = stats.path.rtt.as_millis() + 1, // round up
        );
    }
}

fn suggest_bandwidth_tuning(
    stats: &ConnectionStats,
    command_stats: CommandStats,
    transport_time: Option<Duration>,
    remote_stats: &ClosedownReportV1,
    config: &Configuration,
    direction: Direction,
) {
    let payload_bytes = command_stats.payload_bytes;
    if payload_bytes == 0 {
        return;
    }

    let rtt_ms = u64::try_from(stats.path.rtt.as_millis()).unwrap_or(0);
    let rtt_ms = if rtt_ms == 0 {
        u64::from(config.rtt)
    } else {
        rtt_ms
    };
    if rtt_ms == 0 {
        return;
    }

    let (sender_cwnd, current_bw, sender_frame_stats) = match direction {
        Direction::ClientToServer => (stats.path.cwnd, config.tx(), &stats.frame_tx),
        Direction::ServerToClient => (remote_stats.cwnd.0, config.rx(), &stats.frame_rx),
        Direction::Both => return,
    };
    if sender_cwnd == 0 || current_bw == 0 {
        return;
    }
    let blocked_data = sender_frame_stats.data_blocked;
    let blocked_stream = sender_frame_stats.stream_data_blocked;
    let flow_control_blocked = blocked_data > 0 || blocked_stream > 0;

    let bw_from_cwnd = {
        let numerator = u128::from(sender_cwnd).saturating_mul(1000);
        let denominator = u128::from(rtt_ms);
        let bw = (numerator + denominator - 1) / denominator; // ceil
        u64::try_from(bw).unwrap_or(u64::MAX)
    };

    // Also consider the observed average rate, if available.
    let bw_from_average = DataRate::new(payload_bytes, transport_time)
        .byte_rate()
        .and_then(|r| u64::try_from(r.ceil() as u128).ok())
        .unwrap_or(0);

    // Add some headroom: flow control updates are not continuous, so "exact BDP" can still stall.
    let mut suggested_bw = bw_from_cwnd.max(bw_from_average);
    suggested_bw =
        u64::try_from((u128::from(suggested_bw).saturating_mul(5) + 3) / 4).unwrap_or(u64::MAX);

    // If we observed flow-control blocking, push harder to help the next run converge in fewer iterations.
    if flow_control_blocked && suggested_bw <= current_bw {
        suggested_bw = current_bw.saturating_mul(2);
    }

    // Don't suggest decreases; this flag is about "fastest transfer" tuning.
    suggested_bw = suggested_bw.max(current_bw);

    let flag = match direction {
        Direction::ClientToServer => "--tx",
        Direction::ServerToClient => "--rx",
        Direction::Both => unreachable!(),
    };

    let rtt_str = if stats.path.rtt.as_millis() == 0 {
        config.rtt_duration().human_duration().to_string()
    } else {
        stats.path.rtt.human_duration().to_string()
    };
    let cwnd_str = sender_cwnd.human_count_bytes();
    let suggested_str = suggested_bw.to_eng(4);
    let current_str = current_bw.to_eng(4);

    let increase_substantial = suggested_bw > current_bw.saturating_mul(11) / 10;
    if !flow_control_blocked && !increase_substantial {
        return;
    }

    if flow_control_blocked {
        warn!(
            "Observed flow-control blocking (DATA_BLOCKED={blocked_data}, STREAM_DATA_BLOCKED={blocked_stream}); measured RTT {rtt_str}, sender cwnd {cwnd_str}; for better performance, next time try {flag} {suggested_str} (current {current_str})"
        );
    } else {
        warn!(
            "Measured RTT {rtt_str}, sender cwnd {cwnd_str}; for better performance, next time try {flag} {suggested_str} (current {current_str})"
        );
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::DataRate;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    #[test]
    fn unknown() {
        let r = DataRate::new(1234, None);
        assert_eq!(format!("{r}"), "unknown");
    }
    #[test]
    fn zero() {
        let r = DataRate::new(1234, Some(Duration::from_secs(0)));
        assert_eq!(format!("{r}"), "unknown");
    }

    fn test_case(bytes: u64, time: u64, expect: &str) {
        let r = DataRate::new(bytes, Some(Duration::from_secs(time)));
        assert_eq!(format!("{r}"), expect);
    }
    #[test]
    fn valid() {
        test_case(42, 1, "42B/s");
        test_case(1234, 1, "1.2kB/s");
        test_case(10_000_000_000, 500, "20MB/s");
        test_case(1_000_000_000_000_000, 1234, "810.37GB/s");
    }
}
