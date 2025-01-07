//! Individual configured values
// (c) 2024 Ross Younger

use crate::cli::styles::{INFO, RESET};
use figment::{Metadata, Profile, Source};

#[derive(Debug, Clone, PartialEq)]
/// A setting we read from a config file
pub(crate) struct Setting {
    /// where the value came from
    pub source: String,
    /// line number within the value
    pub line_number: usize,
    /// the setting data itself (not parsed; we assert nothing beyond the parser has applied the ssh quoting logic)
    pub args: Vec<String>,
}

impl Setting {
    pub(crate) fn first_arg(&self) -> String {
        self.args.first().cloned().unwrap_or_else(String::new)
    }
}

///////////////////////////////////////////////////////////////////////////////////////

/// Wraps a Setting into something Figment can deal with
pub(super) struct ValueProvider<'a> {
    key: &'a String,
    value: &'a Setting,
    profile: &'a Profile,
}

impl<'a> ValueProvider<'a> {
    pub(super) fn new(key: &'a String, value: &'a Setting, profile: &'a Profile) -> Self {
        Self {
            key,
            value,
            profile,
        }
    }
}

impl figment::Provider for ValueProvider<'_> {
    fn metadata(&self) -> figment::Metadata {
        Metadata::from(
            "configuration file",
            Source::Custom(format!(
                "{src} (line {line})",
                src = self.value.source,
                line = self.value.line_number
            )),
        )
        .interpolater(|profile, path| {
            let key = path.to_vec();
            format!(
                "key `{INFO}{key}{RESET}` of host `{INFO}{profile}{RESET}`",
                key = key.join(".")
            )
        })
    }

    fn data(
        &self,
    ) -> std::result::Result<
        figment::value::Map<figment::Profile, figment::value::Dict>,
        figment::Error,
    > {
        use figment::value::{Dict, Empty, Value};
        let mut dict = Dict::new();
        let value: Value = match self.value.args.len() {
            0 => Empty::Unit.into(),
            1 => self.value.args.first().unwrap().clone().into(),
            _ => self.value.args.clone().into(),
        };
        let _ = dict.insert(self.key.clone(), value);
        Ok(self.profile.collect(dict))
    }

    fn profile(&self) -> Option<Profile> {
        Some(self.profile.clone())
    }
}
