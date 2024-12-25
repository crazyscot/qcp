//! CLI helper - Address family
// (c) 2024 Ross Younger

use std::str::FromStr;

use figment::error::{Actual, OneOf};
use serde::{de, Deserialize, Serialize};

/// Representation of an IP address family
///
/// This is a local type with special parsing semantics and aliasing to take part in the config/CLI system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")] // to match clap::ValueEnum
pub enum AddressFamily {
    /// IPv4
    #[value(alias("4"), alias("inet4"))]
    Inet,
    /// IPv6
    #[value(alias("6"))]
    Inet6,
    /// We don't mind what type of IP address
    Any,
}

impl FromStr for AddressFamily {
    type Err = figment::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lc = s.to_ascii_lowercase();
        match lc.as_str() {
            "4" | "inet" | "inet4" => Ok(AddressFamily::Inet),
            "6" | "inet6" => Ok(AddressFamily::Inet6),
            "any" => Ok(AddressFamily::Any),
            _ => Err(figment::error::Kind::InvalidType(
                Actual::Str(s.into()),
                OneOf(&["inet", "4", "inet6", "6"]).to_string(),
            )
            .into()),
        }
    }
}

impl<'de> Deserialize<'de> for AddressFamily {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod test {
    use std::str::FromStr;

    use super::AddressFamily;

    #[test]
    fn serialize() {
        let a = AddressFamily::Inet;
        let b = AddressFamily::Inet6;
        let c = AddressFamily::Any;

        let aa = serde_json::to_string(&a);
        let bb = serde_json::to_string(&b);
        let cc = serde_json::to_string(&c);
        assert_eq!(aa.unwrap(), "\"inet\"");
        assert_eq!(bb.unwrap(), "\"inet6\"");
        assert_eq!(cc.unwrap(), "\"any\"");
    }

    #[test]
    fn deser_str() {
        use AddressFamily::*;
        for (str, expected) in &[
            ("4", Inet),
            ("inet", Inet),
            ("inet4", Inet),
            ("6", Inet6),
            ("inet6", Inet6),
            ("any", Any),
        ] {
            let raw = AddressFamily::from_str(str).expect(str);
            let json = format!(r#""{str}""#);
            let output = serde_json::from_str::<AddressFamily>(&json).expect(str);
            assert_eq!(raw, *expected);
            assert_eq!(output, *expected);
        }
    }

    #[test]
    fn deser_invalid() {
        for s in &["true", "5", r#""5""#, "-1", r#""42"#, r#""string"#] {
            let _ = serde_json::from_str::<AddressFamily>(s).expect_err(s);
        }
    }
}
