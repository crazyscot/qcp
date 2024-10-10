// Statistics processing and output
// (c) 2024 Ross Younger

use human_repr::{HumanCount, HumanDuration, HumanThroughput};
use quinn::ConnectionStats;
use std::{fmt::Display, time::Duration};
use tracing::{info, warn};

use crate::cli::CliArgs;

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
) {
    if payload_bytes != 0 {
        let size = payload_bytes.human_count_bytes();
        let rate = crate::util::stats::DataRate::new(payload_bytes, transport_time);
        let transport_time_str =
            transport_time.map_or("unknown".to_string(), |d| d.human_duration().to_string());
        info!("Transferred {size} in {transport_time_str}; average {rate}");
    }
    if stats.path.congestion_events > 0 {
        warn!(
            "Congestion events: {}",
            stats.path.congestion_events.human_count_bare()
        );
    }
    if args.statistics {
        info!("Sent packets: {}", stats.path.sent_packets);
    }
    if stats.path.lost_packets > 0 {
        #[allow(clippy::cast_precision_loss)]
        let pct = 100. * stats.path.lost_packets as f64 / stats.path.sent_packets as f64;
        warn!(
            "Lost packets: {count}/{total} ({pct:.2}%, for {bytes})",
            count = stats.path.lost_packets.human_count_bare(),
            total = stats.path.sent_packets,
            bytes = stats.path.lost_bytes.human_count_bytes(),
        );
    }

    let total_bytes = stats.udp_tx.bytes + stats.udp_rx.bytes;
    if args.statistics {
        info!(
            "Path MTU {pmtu}, round-trip time {rtt}",
            pmtu = stats.path.current_mtu,
            rtt = stats.path.rtt.human_duration(),
        );
        info!(
            "{tx} datagrams sent, {rx} received, {bhd} black holes detected",
            tx = stats.udp_tx.datagrams.human_count_bare(),
            rx = stats.udp_rx.datagrams.human_count_bare(),
            bhd = stats.path.black_holes_detected,
        );
        if payload_bytes != 0 {
            #[allow(clippy::cast_precision_loss)]
            let overhead_pct = 100. * (total_bytes - payload_bytes) as f64 / payload_bytes as f64;
            info!(
                "{} total bytes transferred for {} bytes payload  ({:.2}% overhead)",
                total_bytes, payload_bytes, overhead_pct
            );
        }
    }
    if stats.path.rtt.as_millis() > args.rtt.into() {
        warn!(
            "Measured path RTT {rtt_measured:?} was greater than configuration; for better performance, next time try --rtt {rtt_param}",
            rtt_measured = stats.path.rtt,
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
