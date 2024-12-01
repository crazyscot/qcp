//! Serialization helper type - u64 parseable by humanize_rs
// (c) 2024 Ross Younger

use std::{fmt, marker::PhantomData, ops::Deref, str::FromStr};

use anyhow::Context as _;
use humanize_rs::bytes::Bytes;
use serde::{
    de::{self, Visitor},
    Deserialize, Serialize,
};

/// Newtype wrapper to u64 that adds a flexible deserializer via `humanize_rs::bytes::Bytes<u64>`

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(from = "IntOrString<HumanU64>", into = "u64")]
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

impl FromStr for HumanU64 {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(
            Bytes::from_str(s)
                .with_context(|| "parsing bytes string")?
                .size(),
        ))
    }
}

impl From<u64> for HumanU64 {
    fn from(value: u64) -> Self {
        Self::new(value)
    }
}

/// Deserialization helper for types which might reasonably be expressed as an
/// integer or a string.
///
/// This is a Visitor that forwards string types to T's `FromStr` impl and
/// forwards int types to T's `From<u64>` or `From<i64>` impls. The `PhantomData` is to
/// keep the compiler from complaining about T being an unused generic type
/// parameter. We need T in order to know the Value type for the Visitor
/// impl.
#[allow(missing_debug_implementations)]
pub struct IntOrString<T>(pub PhantomData<fn() -> T>);

impl<'de, T> Visitor<'de> for IntOrString<T>
where
    T: Deserialize<'de> + From<u64> + FromStr,
    <T as FromStr>::Err: std::fmt::Display,
{
    type Value = T;
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("int or string")
    }

    fn visit_str<E>(self, value: &str) -> Result<T, E>
    where
        E: de::Error,
    {
        T::from_str(value).map_err(de::Error::custom)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(T::from(value))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(T::from(value.try_into().map_err(de::Error::custom)?))
    }
}

impl<'de> serde::Deserialize<'de> for HumanU64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(IntOrString(PhantomData))
    }
}

#[cfg(test)]
impl rand::prelude::Distribution<HumanU64> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> HumanU64 {
        rng.gen::<u64>().into()
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
    fn deser_raw_int() {
        let foo: HumanU64 = serde_json::from_str("12345").unwrap();
        assert_eq!(*foo, 12345);
    }

    #[test]
    fn serde_test() {
        let bw = HumanU64::new(42);
        assert_tokens(&bw, &[Token::U64(42)]);
    }

    #[test]
    fn from_int() {
        let result = HumanU64::from(12345);
        assert_eq!(*result, 12345);
    }
    #[test]
    fn from_str() {
        let result = HumanU64::from_str("12345k").unwrap();
        assert_eq!(*result, 12_345_000);
    }
}
