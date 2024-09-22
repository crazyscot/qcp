// Statistics processing
// (c) 2024 Ross Younger

use human_repr::HumanThroughput;
use std::{fmt::Display, time::Duration};

/// Human friendly output helper
pub struct DataRate {
    /// Bytes per second; if None, we were unable to compute a rate.
    rate: Option<f64>,
}

impl DataRate {
    pub fn new(bytes: u64, time: Option<Duration>) -> Self {
        match time {
            None => Self { rate: None },
            Some(time) if time.is_zero() => Self { rate: None }, // divide by zero is not meaningful
            Some(time) => Self {
                rate: Some((bytes as f64) / time.as_secs_f64()),
            },
        }
    }
    pub fn byte_rate(&self) -> Option<f64> {
        self.rate
    }
    pub fn bit_rate(&self) -> Option<f64> {
        self.rate.map(|r| r * 8.)
    }
}

impl Display for DataRate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.bit_rate() {
            None => f.write_str("unknown"),
            Some(rate) => rate.human_throughput("bit").fmt(f),
        }
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
        test_case(42, 1, "336bit/s");
        test_case(1234, 1, "9.9kbit/s");
        test_case(10000000000, 500, "160Mbit/s");
        test_case(1000000000000000, 1234, "6.48Tbit/s");
    }
}
