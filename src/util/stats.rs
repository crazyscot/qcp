//! Statistics processing and output
// (c) 2024 Ross Younger

use human_repr::{HumanCount, HumanDuration, HumanThroughput};
use num_format::ToFormattedString as _;
use quinn::ConnectionStats;
use std::{cmp, fmt::Display, time::Duration};
use tracing::{info, warn};

use crate::{cli::CliArgs, protocol::control::ClosedownReport};

/// Human friendly output helper
#[derive(Debug, Clone, Copy)]
pub struct DataRate {
    /// Bytes per second; if None, we were unable to compute a rate.
    rate: Option<f64>,
}

impl DataRate {
    /// Standard constructor
    #[must_use]
    pub fn new(bytes: u64, time: Option<Duration>) -> Self {
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
    pub fn byte_rate(&self) -> Option<f64> {
        self.rate
    }
    /// Converting accessor
    #[must_use]
    pub fn bit_rate(&self) -> Option<f64> {
        self.rate.map(|r| r * 8.)
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

pub(crate) fn output_statistics(
    args: &CliArgs,
    stats: &ConnectionStats,
    payload_bytes: u64,
    transport_time: Option<Duration>,
    remote_stats: ClosedownReport,
) {
    let locale = &num_format::Locale::en;
    if payload_bytes != 0 {
        let size = payload_bytes.human_count_bytes();
        let rate = crate::util::stats::DataRate::new(payload_bytes, transport_time);
        let transport_time_str =
            transport_time.map_or("unknown".to_string(), |d| d.human_duration().to_string());
        info!("Transferred {size} in {transport_time_str}; average {rate}");
    }
    if args.statistics {
        info!(
            "Total packets sent: {} by us; {} by remote",
            stats.path.sent_packets.to_formatted_string(locale),
            remote_stats.sent_packets.to_formatted_string(locale),
        );
    }
    let congestion = stats.path.congestion_events + remote_stats.congestion_events;
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
    if remote_stats.lost_packets > 0 {
        #[allow(clippy::cast_precision_loss)]
        let pct = 100. * remote_stats.lost_packets as f64 / remote_stats.sent_packets as f64;
        warn!(
            "Remote lost packets: {count}/{total} ({pct:.2}%, for {bytes})",
            count = remote_stats.lost_packets.human_count_bare(),
            total = remote_stats.sent_packets.human_count_bare(),
            bytes = remote_stats.lost_bytes.human_count_bytes(),
        );
    }

    let sender_sent_bytes = cmp::max(stats.udp_tx.bytes, remote_stats.sent_bytes);
    if args.statistics {
        let cwnd = cmp::max(stats.path.cwnd, remote_stats.cwnd);
        info!(
            "Path MTU {pmtu}, round-trip time {rtt}, final congestion window {cwnd}",
            pmtu = stats.path.current_mtu,
            rtt = stats.path.rtt.human_duration(),
            cwnd = cwnd.to_formatted_string(locale),
        );
        let black_holes = stats.path.black_holes_detected + remote_stats.black_holes_detected;
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
    if stats.path.rtt.as_millis() > args.bandwidth.rtt.into() {
        warn!(
            "Measured path RTT {rtt_measured:?} was greater than configuration {rtt_arg}; for better performance, next time try --rtt {rtt_param}",
            rtt_measured = stats.path.rtt,
            rtt_arg = args.bandwidth.rtt,
            rtt_param = stats.path.rtt.as_millis()+1, // round up
        );
    }
}

#[cfg(test)]
mod tests {
    use super::DataRate;
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
