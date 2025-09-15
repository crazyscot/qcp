//! Serialization helpers
// (c) 2024 Ross Younger

use std::marker::PhantomData;

use serde::{Deserializer, Serializer, de::Visitor};

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
pub trait DeserializeEnum
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

    /// Serialization helper for use with Optionalify `#[deftly(serialize_with = ...)]`.
    // The large-Err warning cannot be helped, it's forced by Figment
    #[allow(clippy::result_large_err)]
    fn to_string_wrapper(&self) -> Result<String, figment::Error> {
        Ok(self.as_ref().to_string())
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
