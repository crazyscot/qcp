//! CLI generic serialization helpers
// (c) 2024 Ross Younger

use std::{fmt, marker::PhantomData, str::FromStr};

use serde::{de, de::Visitor, Deserialize};

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
    T: Deserialize<'de> + TryFrom<u64> + FromStr,
    <T as FromStr>::Err: std::fmt::Display,
    <T as TryFrom<u64>>::Error: std::fmt::Display,
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
        T::try_from(value).map_err(de::Error::custom)
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        let u = u64::try_from(value).map_err(de::Error::custom)?;
        T::try_from(u).map_err(de::Error::custom)
    }
}
