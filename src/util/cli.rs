// CLI argument
// (c) 2024 Ross Younger

use std::{fmt::Display, str::FromStr};

/// Represents a number or a contiguous range of positive integers
#[derive(Debug, Clone, Copy)]
pub struct PortRange {
    /// First number in the range
    pub begin: u16,
    /// Last number in the range.
    /// The caller defines whether the range is inclusive or exclusive of `end`.
    pub end: u16,
}

impl Display for PortRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.begin == self.end {
            f.write_fmt(format_args!("{}", self.begin))
        } else {
            f.write_fmt(format_args!("{}-{}", self.begin, self.end))
        }
    }
}

impl FromStr for PortRange {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(n) = s.parse::<u16>() {
            // case 1: it's a number
            // port 0 is allowed here (with the usual "unspecified" semantics), the user may know what they're doing.
            return Ok(Self { begin: n, end: n });
        }
        // case 2: it's a range
        if let Some((a, b)) = s.split_once('-') {
            let aa = a.parse();
            let bb = b.parse();
            if aa.is_ok() && bb.is_ok() {
                let aa = aa.unwrap_or_default();
                let bb = bb.unwrap_or_default();
                anyhow::ensure!(aa != 0, "0 is not valid in a port range");
                anyhow::ensure!(aa <= bb, "invalid range");
                return Ok(Self { begin: aa, end: bb });
            }
            // else failed to parse
        }
        // else failed to parse
        anyhow::bail!("failed to parse range");
    }
}

/// Parse helper for Duration fields specified in seconds
pub fn parse_duration(arg: &str) -> Result<std::time::Duration, std::num::ParseIntError> {
    let seconds = arg.parse()?;
    Ok(std::time::Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    type Uut = super::PortRange;

    #[test]
    fn output_single() {
        let uut = Uut {
            begin: 123,
            end: 123,
        };
        assert_eq!(format!("{uut}"), "123");
    }
    #[test]
    fn output_range() {
        let uut = Uut {
            begin: 123,
            end: 456,
        };
        assert_eq!(format!("{uut}"), "123-456");
    }
    #[test]
    fn parse_single() {
        let uut = Uut::from_str("1234").unwrap();
        assert_eq!(uut.begin, 1234);
        assert_eq!(uut.end, 1234);
    }
    #[test]
    fn parse_range() {
        let uut = Uut::from_str("1234-2345").unwrap();
        assert_eq!(uut.begin, 1234);
        assert_eq!(uut.end, 2345);
    }
    #[test]
    fn invalid_range() {
        let _ = Uut::from_str("1000-999").expect_err("should have failed");
    }
    #[test]
    fn invalid_negative() {
        let _ = Uut::from_str("-500").expect_err("should have failed");
    }
    #[test]
    fn port_range_not_zero() {
        let _ = Uut::from_str("0-1000").expect_err("should have failed");
    }
}
