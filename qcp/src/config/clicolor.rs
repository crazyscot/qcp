//! Config provider for CLICOLOR environment variables
//!
//! See [https://bixense.com/clicolors/](https://bixense.com/clicolors/) for more information.
// (c) 2025 Ross Younger

use figment::{Metadata, Provider};

pub(super) struct Env {}
impl Env {
    const META_NAME: &str = "environment variable(s)";
}

impl Provider for Env {
    fn metadata(&self) -> Metadata {
        figment::Metadata::named(Self::META_NAME)
    }

    fn data(
        &self,
    ) -> std::result::Result<
        figment::value::Map<figment::Profile, figment::value::Dict>,
        figment::Error,
    > {
        let mut dict = figment::value::Dict::new();
        let value = if std::env::var("NO_COLOR").is_ok() {
            Some("never")
        } else if std::env::var("CLICOLOR_FORCE").is_ok() {
            Some("always")
        } else if let Ok(v) = std::env::var("CLICOLOR") {
            if v.is_empty() || v == "0" {
                Some("never")
            } else {
                Some("auto")
            }
        } else {
            None
        };
        if let Some(v) = value {
            let _ = dict.insert("color".into(), v.into());
        }
        Ok(figment::Profile::Default.collect(dict))
    }
}

// Tests for this module are in `qcp_unsafe_tests::clicolor`.
