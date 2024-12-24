//! Serialization helper type - u64 parseable by humanize_rs
// (c) 2024 Ross Younger

use std::{ops::Deref, str::FromStr};

use humanize_rs::bytes::Bytes;
use serde::{
    de::{self, Error as _},
    Serialize,
};

/// An integer field that may also be expressed using SI notation (k, M, G, etc).
/// For example, `1k` and `1000` are the same.
///
/// (Nerdy description: This is a newtype wrapper to `u64` that adds a flexible deserializer via `humanize_rs::bytes::Bytes<u64>`.)

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(from = "String", into = "String")]
pub struct HumanU64(pub u64);

impl HumanU64 {
    /// standard constructor
    #[must_use]
    pub fn new(value: u64) -> Self {
        Self(value)
    }
}

impl Deref for HumanU64 {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<HumanU64> for u64 {
    fn from(value: HumanU64) -> Self {
        value.0
    }
}

impl From<HumanU64> for String {
    fn from(value: HumanU64) -> Self {
        format!("{}", *value)
    }
}

impl FromStr for HumanU64 {
    type Err = figment::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        use figment::error::Error as FigmentError;
        Ok(Self::new(
            Bytes::from_str(s)
                .map_err(|_| {
                    FigmentError::invalid_value(
                        de::Unexpected::Str(s),
                        &"an integer with optional units (examples: `100`, `10M`, `42k`)",
                    )
                })?
                .size(),
        ))
    }
}

impl From<u64> for HumanU64 {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

impl<'de> serde::Deserialize<'de> for HumanU64 {
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
    use std::str::FromStr as _;

    use serde_test::{assert_tokens, Token};

    use super::HumanU64;

    fn test_deser_str(s: &str, n: u64) {
        let foo: HumanU64 = serde_json::from_str(s).unwrap();
        assert_eq!(*foo, n);
    }

    #[test]
    fn deser_number_string() {
        test_deser_str("\"12345\"", 12345);
    }

    #[test]
    fn deser_human() {
        test_deser_str("\"100k\"", 100_000);
    }

    #[test]
    fn serde_test() {
        let bw = HumanU64::new(42);
        assert_tokens(&bw, &[Token::Str("42")]);
    }

    #[test]
    fn from_int() {
        let result = HumanU64::new(12345);
        assert_eq!(*result, 12345);
    }
    #[test]
    fn from_str() {
        let result = HumanU64::from_str("12345k").unwrap();
        assert_eq!(*result, 12_345_000);
    }
}
