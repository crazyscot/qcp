/// CLI argument helper - PortRange
// (c) 2024 Ross Younger
use serde::Serialize;
use std::{fmt::Display, str::FromStr};

use super::cli::IntOrString;

/// A range of UDP port numbers.
///
/// Port 0 is allowed with the usual meaning ("any available port"), but 0 may not form part of a range.
///
/// In a configuration file, a range must be specified as a string. For example:
/// ```toml
/// remote_port=60000         # a single port can be an integer
/// remote_port="60000"       # a single port can also be a string
/// remote_port="60000-60010" # a range must be specified as a string
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(from = "IntOrString<PortRange>", into = "String")]
pub struct PortRange {
    /// First number in the range
    pub begin: u16,
    /// Last number in the range, inclusive.
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

impl From<PortRange> for String {
    fn from(value: PortRange) -> Self {
        value.to_string()
    }
}

impl FromStr for PortRange {
    type Err = figment::Error;

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
                if aa != 0 && aa <= bb {
                    return Ok(Self { begin: aa, end: bb });
                }
                // else invalid
            }
            // else failed to parse
        }
        // else failed to parse
        Err(figment::error::Kind::Message(format!("invalid port range \"{s}\"")).into())
    }
}

impl From<u64> for PortRange {
    fn from(value: u64) -> Self {
        #[allow(clippy::cast_possible_truncation)]
        let v = value as u16;
        PortRange { begin: v, end: v }
    }
}

impl<'de> serde::Deserialize<'de> for PortRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(IntOrString(std::marker::PhantomData))
    }
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
