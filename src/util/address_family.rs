//! CLI helper - Address family
// (c) 2024 Ross Younger

use std::fmt::Display;
use std::marker::PhantomData;
use std::str::FromStr;

use figment::error::Actual;
use serde::Serialize;

use crate::util::cli::IntOrString;

/// Representation an IP address family
///
/// This is a local type with special parsing semantics to take part in the config/CLI system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AddressFamily {
    /// IPv4
    #[value(name = "4")]
    V4,
    /// IPv6
    #[value(name = "6")]
    V6,
    /// We don't mind what type of IP address
    Any,
}

impl From<AddressFamily> for u8 {
    fn from(value: AddressFamily) -> Self {
        match value {
            AddressFamily::V4 => 4,
            AddressFamily::V6 => 6,
            AddressFamily::Any => 0,
        }
    }
}

impl Serialize for AddressFamily {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match *self {
            AddressFamily::Any => serializer.serialize_str("any"),
            t => serializer.serialize_u8(u8::from(t)),
        }
    }
}

impl Display for AddressFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if *self == AddressFamily::Any {
            write!(f, "any")
        } else {
            write!(f, "{}", u8::from(*self))
        }
    }
}

impl FromStr for AddressFamily {
    type Err = figment::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "4" {
            Ok(AddressFamily::V4)
        } else if s == "6" {
            Ok(AddressFamily::V6)
        } else if s == "0" || s == "any" {
            Ok(AddressFamily::Any)
        } else {
            Err(figment::error::Kind::InvalidType(Actual::Str(s.into()), "4 or 6".into()).into())
        }
    }
}

impl TryFrom<u64> for AddressFamily {
    type Error = figment::Error;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            4 => Ok(AddressFamily::V4),
            6 => Ok(AddressFamily::V6),
            0 => Ok(AddressFamily::Any),
            _ => Err(figment::error::Kind::InvalidValue(
                Actual::Unsigned(value.into()),
                "4 or 6".into(),
            )
            .into()),
        }
    }
}

impl<'de> serde::Deserialize<'de> for AddressFamily {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(IntOrString(PhantomData))
    }
}

#[cfg(test)]
mod test {
    use super::AddressFamily;

    #[test]
    fn serialize() {
        let a = AddressFamily::V4;
        let b = AddressFamily::V6;

        let aa = serde_json::to_string(&a);
        let bb = serde_json::to_string(&b);
        assert_eq!(aa.unwrap(), "4");
        assert_eq!(bb.unwrap(), "6");
    }

    #[test]
    fn deser_str() {
        let a: AddressFamily = serde_json::from_str(r#" "4" "#).unwrap();
        assert_eq!(a, AddressFamily::V4);
        let a: AddressFamily = serde_json::from_str(r#" "6" "#).unwrap();
        assert_eq!(a, AddressFamily::V6);
    }

    #[test]
    fn deser_int() {
        let a: AddressFamily = serde_json::from_str("4").unwrap();
        assert_eq!(a, AddressFamily::V4);
        let a: AddressFamily = serde_json::from_str("6").unwrap();
        assert_eq!(a, AddressFamily::V6);
    }

    #[test]
    fn deser_invalid() {
        let _ = serde_json::from_str::<AddressFamily>("true").unwrap_err();
        let _ = serde_json::from_str::<AddressFamily>("5").unwrap_err();
        let _ = serde_json::from_str::<AddressFamily>(r#" "5" "#).unwrap_err();
        let _ = serde_json::from_str::<AddressFamily>("-1").unwrap_err();
        let _ = serde_json::from_str::<AddressFamily>(r#" "42" "#).unwrap_err();
        let _ = serde_json::from_str::<AddressFamily>(r#" "string" "#).unwrap_err();
    }
}
