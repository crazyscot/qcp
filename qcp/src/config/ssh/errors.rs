//! Error output helpers
// (c) 2024 Ross Younger

use figment::error::{Kind, OneOf};
use thiserror::Error;

/// A newtype wrapper implementing `Display` for errors originating from this module
#[derive(Debug, Error, PartialEq)]
pub(crate) struct ConfigFileError(#[source] Box<figment::Error>);

impl From<figment::Error> for ConfigFileError {
    fn from(e: figment::Error) -> Self {
        Self(Box::new(e))
    }
}
impl std::ops::Deref for ConfigFileError {
    type Target = figment::Error;

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
        let e = self.0.as_ref();
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

/// An iterator over all errors in an [`SshConfigError`]
pub(crate) struct IntoIter(<figment::Error as std::iter::IntoIterator>::IntoIter);
impl Iterator for IntoIter {
    type Item = ConfigFileError;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(std::convert::Into::into)
    }
}

impl IntoIterator for ConfigFileError {
    type Item = ConfigFileError;
    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.0.into_iter())
    }
}
