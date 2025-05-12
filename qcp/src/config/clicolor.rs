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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::{Configuration, cli::styles::ColourMode, config::Manager};
    use rusty_fork::rusty_fork_test;

    fn test_body<T: FnOnce()>(setter: T, expected: ColourMode, check_meta: bool) {
        #[allow(unsafe_code)]
        unsafe {
            std::env::remove_var("NO_COLOR");
            std::env::remove_var("CLICOLOR");
            std::env::remove_var("CLICOLOR_FORCE");
            setter();
        }
        let mut mgr = Manager::new(None, true, false);
        mgr.apply_system_default();
        let result = mgr.get::<Configuration>().unwrap();
        assert_eq!(result.color, expected.into());

        if check_meta {
            let val = mgr.data.find_value("color").unwrap();
            let meta = mgr.data.get_metadata(val.tag()).unwrap();
            assert!(meta.name.contains("environment"));
        }
    }

    macro_rules! test_case {
        ($name:ident, $var:literal, $val:literal, $expected:ident, $check_meta:expr) => {
            rusty_fork_test! {
                #[test]
                fn $name() {
                    test_body(
                        || {
                            #[allow(unsafe_code)]
                            unsafe {
                                std::env::set_var($var, $val);
                            }
                        },
                        ColourMode::$expected,
                        $check_meta
                    );
                }
            }
        };
    }

    test_case!(force, "CLICOLOR_FORCE", "1", Always, true);
    test_case!(auto, "CLICOLOR", "1", Auto, true);
    test_case!(off, "CLICOLOR", "0", Never, true);
    test_case!(no, "NO_COLOR", "1", Never, true);
    test_case!(nothing, "nothing", "no", Auto, false);
}
