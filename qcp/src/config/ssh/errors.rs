//! Error output helpers
// (c) 2024 Ross Younger

use figment::error::{Kind, OneOf};
use thiserror::Error;

/// A newtype wrapper implementing `Display` for errors originating from this module
#[derive(Debug, Error, PartialEq)]
pub struct ConfigFileError(#[source] Box<figment::Error>);

impl From<figment::Error> for ConfigFileError {
    fn from(e: figment::Error) -> Self {
        Self(Box::new(e))
    }
}
impl std::ops::Deref for ConfigFileError {
    type Target = figment::Error;

    /// Returns a reference to the inner error
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ConfigFileError {
    fn rewrite_expected_type(s: &str) -> String {
        match s {
            "a boolean" => format!(
                "a boolean ({})",
                OneOf(&["yes", "no", "true", "false", "1", "0"])
            ),
            _ => s.to_owned(),
        }
    }

    fn fmt_kind(kind: &Kind, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match kind {
            Kind::InvalidType(v, exp) => write!(
                f,
                "invalid type: found {v}, expected {exp}",
                exp = Self::rewrite_expected_type(exp)
            ),
            Kind::UnknownVariant(v, exp) => {
                write!(f, "unknown variant: found {v}, expected {}", OneOf(exp))
            }
            _ => std::fmt::Display::fmt(&kind, f),
        }
    }
}

impl std::fmt::Display for ConfigFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let e = self;
        Self::fmt_kind(&e.kind, f)?;

        if let (Some(profile), Some(md)) = (&e.profile, &e.metadata) {
            if !e.path.is_empty() {
                let key = md.interpolate(profile, &e.path);
                write!(f, " for {key}")?;
            }
        }

        if let Some(md) = &e.metadata {
            if let Some(source) = &md.source {
                write!(f, " at {source}")?;
            } else {
                write!(f, " in {}", md.name)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::config::ssh::errors::ConfigFileError;
    use figment::error::{Actual, Kind};
    use pretty_assertions::assert_eq;

    #[test]
    fn error_invalid_type() {
        let err = ConfigFileError::from(figment::Error::from(Kind::InvalidType(
            Actual::Str("string".into()),
            "a boolean".to_string(),
        )));
        assert!(
            err.to_string()
                .contains("invalid type: found string \"string\", expected a boolean")
        );
    }
    #[test]
    fn error_unknown_variant() {
        let knowns = &["a", "b"];
        let err = ConfigFileError::from(figment::Error::from(Kind::UnknownVariant(
            "string".to_string(),
            knowns,
        )));
        assert_eq!(
            err.to_string(),
            "unknown variant: found string, expected `a` or `b`"
        );
    }
    #[test]
    fn error_unsupported() {
        let err = ConfigFileError::from(figment::Error::from(Kind::Unsupported(Actual::Str(
            "abc".into(),
        ))));
        assert_eq!(err.to_string(), "unsupported type `string \"abc\"`");
    }
}
