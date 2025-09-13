//! Serialization helpers
// (c) 2024 Ross Younger

use std::{marker::PhantomData, str::FromStr};

use derive_more::with_trait::{Debug, Deref, Display};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Visitor};

/// A wrapping helper type for enums that changes how they are serialized.
///
/// To use it, the enum must have the following properties:
/// - Derives [`strum::EnumString`] and [`strum::VariantNames`]
/// - Derives [`strum::Display`] or declares an equivalent [`Display`]
/// - Declares `#[strum(serialize_all = "lowercase")]`
///
/// To convert the enum into the wrapper, use `from()` or `into()`.
///
/// To convert the wrapper to the enum, dereference it `*var`.
#[derive(Clone, Copy, PartialEq, Debug, Display, derive_more::From, Deref, Serialize)]
#[serde(into = "String")]
pub struct SerializeAsString<E: Clone + Display + FromStr>(
    /// The inner enum type
    pub E,
);

impl<'de, E> Deserialize<'de> for SerializeAsString<E>
where
    E: Clone + Display + FromStr + strum::VariantNames,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let lower = s.to_ascii_lowercase();
        E::from_str(&lower)
            .map_err(|_| serde::de::Error::unknown_variant(&s, E::VARIANTS))
            .map(|v| SerializeAsString(v))
    }
}

impl<E: Clone + Display + FromStr> From<SerializeAsString<E>> for String {
    fn from(value: SerializeAsString<E>) -> Self {
        value.to_string()
    }
}

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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use std::str::FromStr;

    use super::SerializeAsString;
    use crate::protocol::control::CongestionController;
    use pretty_assertions::assert_eq;

    #[test]
    fn derivations() {
        let uut = SerializeAsString(CongestionController::Cubic);
        assert_eq!(uut, uut);
        println!("{uut}");
        println!("{uut:?}");
        println!("{uut:#?}");
        let u2 = uut;
        #[allow(clippy::clone_on_copy)] // explicit clone
        let u3 = uut.clone();
        assert_eq!(uut, u2);
        assert_eq!(uut, u3);
    }

    #[test]
    fn conversions() {
        let t = CongestionController::Bbr;
        let u = SerializeAsString::from(t);
        assert_eq!(*u, CongestionController::Bbr);

        let s = String::from(u);
        assert_eq!(s, "bbr");
        // case insensitive
        let v = CongestionController::from_str("bbr");
        assert_eq!(v.unwrap(), CongestionController::Bbr);
    }

    #[test]
    fn serde() {
        let mode = SerializeAsString(CongestionController::Cubic);
        let j = serde_json::to_string(&mode).unwrap();
        assert_eq!(j, r#""cubic""#);

        let res = serde_json::from_str::<SerializeAsString<CongestionController>>(&j).unwrap();
        assert_eq!(res, mode);

        // case insensitive
        let t2 = r#""CUbiC""#;
        let res = serde_json::from_str::<SerializeAsString<CongestionController>>(t2).unwrap();
        assert_eq!(res, mode);
    }
}
