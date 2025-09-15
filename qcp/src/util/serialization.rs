//! Serialization helpers
// (c) 2024 Ross Younger

use std::{fmt::Display, marker::PhantomData, str::FromStr};

use engineering_repr::EngineeringQuantity;
use serde::{Deserialize as _, Deserializer, Serializer, de::SeqAccess, de::Visitor};

/// String to enum deserialization helper trait (via enumscribe).
///
/// Where it is necessary for an enum to have multiple serializations
/// (e.g. wire protocol and config file parsing), this trait can help.
///
/// If you implement this trait for an enum, then when the enum appears
/// in a struct you can use the trait function in a serde attribute, like this:
/// ```plaintext
/// #[serde(deserialize_with = "CredentialsType::deserialize_str")]
/// ```
///
/// The enum should auto-derive `enumscribe::TryUnscribe`, `strum::VariantNames`
/// and `strum::AsRefStr`.
///
/// You are not expected to need to override the default implementation.
pub trait SerializeEnumAsString
where
    Self: Sized + enumscribe::TryUnscribe + strum::VariantNames + AsRef<str>,
{
    /// String serialize function for an enum.
    fn serialize_str<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_ref())
    }

    /// String deserialize function for an enum.
    fn deserialize_str<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MyVisitor<EE>(PhantomData<EE>);
        impl<EE> Visitor<'_> for MyVisitor<EE>
        where
            EE: enumscribe::TryUnscribe + strum::VariantNames,
        {
            type Value = EE;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a string")
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Self::Value::try_unscribe(v)
                    .ok_or_else(|| serde::de::Error::unknown_variant(v, Self::Value::VARIANTS))
            }
        }
        deserializer.deserialize_str(MyVisitor::<Self>(PhantomData))
    }
    /// String deserialize function for an `Option<some enum>`.
    /// Always returns `Some(enum-type)`.
    fn deserialize_str_optional<'de, D>(deserializer: D) -> Result<Option<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Some(Self::deserialize_str(deserializer)?))
    }
}

/// Helper trait that provides alternative serialization, as a string.
/// For use with `#[serde(serialize_with ..., deserialize_with ...)]`.
pub trait SerializeAsString
where
    Self: FromStr + Display,
    <Self as std::str::FromStr>::Err: std::fmt::Display,
{
    /// Serialization that wraps to Display
    fn serialize_str<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
    /// Deserialization from string
    fn deserialize_str<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MyVisitor<T>(PhantomData<T>);
        impl<T> Visitor<'_> for MyVisitor<T>
        where
            T: FromStr,
            <T as std::str::FromStr>::Err: std::fmt::Display,
        {
            type Value = T;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a string")
            }
            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Self::Value::from_str(s).map_err(|e| serde::de::Error::custom(e))
            }
        }
        deserializer.deserialize_str(MyVisitor::<Self>(PhantomData))
    }
    /// Deserialisation from string for `Option<Type>`
    /// Always returns `Some(type)`.
    fn deserialize_str_optional<'de, D>(deserializer: D) -> Result<Option<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Some(Self::deserialize_str(deserializer)?))
    }
}

/// Serialization helper bridging [`figment::Figment`] and [`crate::derive_deftly_template_Optionalify`]
///
/// You are not expected to need to override the default implementation.
pub trait ToStringForFigment
where
    Self: Display,
{
    /// Serialization helper for use with Optionalify `#[deftly(serialize_with = ...)]`.
    ///
    /// This is an infallible conversion.
    /// Returning &str as its error type is a hack to make it fit neatly into
    /// figment::Error.
    fn to_string_figment(&self) -> Result<String, &str> {
        Ok(self.to_string())
    }
}

/// Deserialization helper allowing a value to appear as either a `String` or a `Vec<String>`
pub(crate) struct StringOrVec {}

impl StringOrVec {
    /// Deserialization of `Vec<String>` from string
    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SVVisitor;
        impl<'de> Visitor<'de> for SVVisitor {
            type Value = Vec<String>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("a string or list of string")
            }
            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(vec![s.to_owned()])
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut v = Vec::new();
                while let Some(it) = seq.next_element()? {
                    v.push(it);
                }
                Ok(v)
            }
        }
        deserializer.deserialize_any(SVVisitor)
    }

    /// Deserialization of `Option<Vec<String>>` from string
    pub(crate) fn deserialize_optional<'de, D>(
        deserializer: D,
    ) -> Result<Option<Vec<String>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Self::deserialize(deserializer).map(Some)
    }
}

/// Helper type for serializing u64 via EngineeringQuantity
pub(crate) struct EQHelper {}
impl EQHelper {
    #[allow(clippy::trivially_copy_pass_by_ref)] // required by serde
    pub(crate) fn serialize<S>(val: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(
            &EngineeringQuantity::<u64>::from(*val)
                .with_precision(0)
                .to_string(),
        )
    }
    pub(crate) fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        EngineeringQuantity::<u64>::deserialize(deserializer).map(Into::into)
    }
    pub(crate) fn deserialize_optional<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
    where
        D: Deserializer<'de>,
    {
        EngineeringQuantity::<u64>::deserialize(deserializer).map(|eq| Some(u64::from(eq)))
    }

    #[allow(clippy::unnecessary_wraps)] // required by serde
    pub(crate) fn to_string_figment(val: &u64) -> Result<String, &str> {
        Ok(EngineeringQuantity::<u64>::from(*val)
            .with_precision(0)
            .to_string())
    }
}
