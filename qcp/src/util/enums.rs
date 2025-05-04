//! Helpers for enums
// (c) 2024 Ross Younger

use std::str::FromStr;

use serde::Deserialize;

/// A newtype wrapper providing shared implementation for enums that can
/// be deserialized from a string.
///
/// This type supports construction via `Into` but not deconstruction;
/// to extract, look at `ConvertibleTo`.
///
/// To use on a clap argument, you may want to set
///    `#[arg(value_parser = clap::builder::EnumValueParser::<YourType>::new().map(DeserializableEnum::<YourType>::from))]`
#[derive(
    Clone,
    Copy,
    PartialEq,
    derive_more::Debug,
    derive_more::Display,
    derive_more::From,
    derive_more::Deref,
    serde::Serialize,
)]
#[serde(transparent)]
pub struct DeserializableEnum<T: clap::ValueEnum>(pub(crate) T);

impl<T> DeserializableEnum<T>
where
    T: clap::ValueEnum,
{
    /// Convert a `DeserializableEnum` into the underlying type.
    pub fn into_inner(self) -> T {
        self.0
    }
}

// Deserialization from config file
impl<'de, T> Deserialize<'de> for DeserializableEnum<T>
where
    T: clap::ValueEnum + Sync + Send + 'static,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        <DeserializableEnum<T> as FromStr>::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// A From-like trait but without the tangle you get from `From` and `Into`.
pub(crate) trait ConvertibleTo<T> {
    fn convert(self) -> T;
}

impl<T> ConvertibleTo<T> for T {
    fn convert(self) -> T {
        self
    }
}

impl<T> ConvertibleTo<T> for DeserializableEnum<T>
where
    T: clap::ValueEnum,
{
    fn convert(self) -> T {
        self.0
    }
}

// I have, at one point, an Option<DeserializableEnum<E>>
// and I want to convert it to an Option<E>.
impl<T> ConvertibleTo<Option<T>> for Option<DeserializableEnum<T>>
where
    T: clap::ValueEnum,
{
    fn convert(self) -> Option<T> {
        self.map(|e| e.0)
    }
}

/// Conversion from string, for clap (CLI)
impl<T> FromStr for DeserializableEnum<T>
where
    T: clap::ValueEnum + Sync + Send + 'static,
{
    type Err = figment::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        <T as clap::ValueEnum>::from_str(s, true)
            .map(DeserializableEnum)
            .map_err(|err| {
                use clap::builder::TypedValueParser as _;
                let parser = clap::builder::EnumValueParser::<T>::new();
                let values = parser.possible_values();
                let mut msg = err;
                if let Some(vv) = values {
                    msg = String::from("expected one of ");
                    let mut aliases = Vec::new();
                    vv.for_each(|v| {
                        let it = v.get_name_and_aliases();
                        it.for_each(|n| {
                            let mut s = String::from('`');
                            s.push_str(n);
                            s.push('`');
                            aliases.push(s);
                        });
                    });
                    msg.push_str(&aliases.join(", "));
                }
                figment::error::Kind::Message(msg).into()
            })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::DeserializableEnum;
    use crate::{cli::styles::ColourMode, util::enums::ConvertibleTo as _};
    use std::str::FromStr as _;

    #[test]
    fn simple_conversions() {
        let a = ColourMode::Auto;
        let b = DeserializableEnum::<ColourMode>::from(a);

        assert_eq!(a, b.convert());
        assert_eq!(a, b.into_inner());

        let c = Some(b);
        let d: Option<ColourMode> = c.convert();
        assert_eq!(d, Some(a));
    }

    #[test]
    fn from_string() {
        let a = ColourMode::Auto;
        let b = DeserializableEnum::<ColourMode>::from_str("auto").unwrap();
        assert_eq!(a, b.convert());
        assert_eq!(a, b.into_inner());

        let c = DeserializableEnum::<ColourMode>::from_str("bad").unwrap_err();
        assert!(c.to_string().contains("expected one of"));
        assert!(c.to_string().contains("auto"));
        assert!(c.to_string().contains("on")); // alias
        assert!(c.to_string().contains("always"));
        assert!(c.to_string().contains("off")); // alias
        assert!(c.to_string().contains("never"));
    }
}
