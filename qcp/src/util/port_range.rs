//! CLI argument helper type - a range of UDP port numnbers.
// (c) 2024 Ross Younger
use serde::{
    Serialize,
    de::{self, Error, Unexpected},
};
use std::{fmt::Display, str::FromStr};

/// A range of UDP port numbers.
///
/// Port 0 is allowed with the usual meaning ("any available port"), but 0 may not form part of a range.
///
/// In a configuration file, a range may specified as an integer or as a pair of ports. For example:
/// ```text
/// remote_port 60000         # a single port
/// remote_port 60000-60010   # a range
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(from = "String", into = "String")]
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

static PR_EXPECTED: &str = "a single port number [0..65535] or a range `a-b`";

impl FromStr for PortRange {
    type Err = figment::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use figment::error::Error as FigmentError;
        if let Ok(n) = s.parse::<u16>() {
            // case 1: it's a number
            // port 0 is allowed here (with the usual "unspecified" semantics), the user may know what they're doing.
            return Ok(Self { begin: n, end: n });
        }
        if let Ok(n) = s.parse::<u64>() {
            // out of range
            return Err(FigmentError::invalid_value(
                Unexpected::Unsigned(n),
                &PR_EXPECTED,
            ));
        }
        // case 2: it's a range
        if let Some((a, b)) = s.split_once('-') {
            let aa = a.parse();
            let bb = b.parse();
            if aa.is_ok() && bb.is_ok() {
                let aa = aa.unwrap_or_default();
                let bb = bb.unwrap_or_default();
                if aa > bb {
                    return Err(FigmentError::custom(format!(
                        "invalid port range `{s}` (must be increasing)"
                    )));
                } else if aa == 0 && bb != 0 {
                    return Err(FigmentError::custom(format!(
                        "invalid port range `{s}` (port 0 means \"any\" so cannot be part of a range)"
                    )));
                }
                return Ok(Self { begin: aa, end: bb });
            }
            // else failed to parse
        }
        // else failed to parse
        Err(FigmentError::invalid_value(
            Unexpected::Str(s),
            &PR_EXPECTED,
        ))
    }
}

impl PortRange {
    pub(crate) fn is_default(self) -> bool {
        self.begin == 0 && self.begin == self.end
    }

    /// Look at the configured and client-requested port ranges, determine if a solution can be found
    /// and return it if so.
    pub(crate) fn combine(self, theirs: PortRange) -> anyhow::Result<PortRange> {
        Ok(if self.is_default() {
            // no config this side; other prefs win
            theirs
        } else if theirs.is_default() {
            // we win by default
            self
        } else {
            // Intersect both preferences. If that results in no solution, report error.
            let begin = std::cmp::max(self.begin, theirs.begin);
            let end = std::cmp::min(self.end, theirs.end);
            anyhow::ensure!(
                begin <= end,
                "requested port range {theirs} could not be satisfied (our config: {self})"
            );
            PortRange { begin, end }
        })
    }
}

impl<'de> serde::Deserialize<'de> for PortRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(|e: figment::Error| {
            if let figment::error::Kind::InvalidValue(_, _) = e.kind {
                de::Error::invalid_value(Unexpected::Str(&s), &PR_EXPECTED)
            } else {
                de::Error::custom(e)
            }
        })
    }
}

impl From<PortRange> for figment::value::Value {
    fn from(value: PortRange) -> Self {
        value.to_string().into()
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::PortRange as ConfigPortRange;
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
        // and convert to a value
        let v = figment::value::Value::from(uut);
        assert_eq!(v.as_str(), Some("1234-2345"));
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
    fn invalid_out_of_range() {
        let _ = Uut::from_str("65537").expect_err("should have failed");
    }
    #[test]
    fn invalid_unparseable() {
        let _ = Uut::from_str("fdsfdsfds").expect_err("should have failed");
    }
    #[test]
    fn port_range_not_zero() {
        let _ = Uut::from_str("0-1000").expect_err("should have failed");
    }

    #[test]
    fn port_range_combine() {
        fn pr(begin: u16, end: u16) -> Uut {
            ConfigPortRange { begin, end }
        }

        let config = pr(42, 88);
        // easy case: defaults
        assert_eq!(Uut::default().combine(config).unwrap(), config);
        assert_eq!(config.combine(Uut::default()).unwrap(), config);

        // overlap one end
        assert_eq!(config.combine(pr(77, 99)).unwrap(), pr(77, 88));
        // overlap the other end
        assert_eq!(config.combine(pr(5, 49)).unwrap(), pr(42, 49));
        // superset
        assert_eq!(config.combine(pr(5, 123)).unwrap(), pr(42, 88));
        // subset
        assert_eq!(config.combine(pr(51, 62)).unwrap(), pr(51, 62));
        // disjoint
        let _ = config.combine(pr(123, 456)).expect_err("failure expected");
    }
}
