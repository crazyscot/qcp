//! Configuration file wrangling
// (c) 2024 Ross Younger

use crate::{
    cli::styles::ColourMode,
    os::{AbstractPlatform as _, Platform},
    util::enums::DeserializableEnum,
};

use super::{ClicolorEnv, Configuration, Configuration_Optional, ssh::ConfigFileError};

use anyhow::Result;
use figment::{Figment, Provider};
use serde::{Deserialize, de::Error};
use std::{
    fmt::Debug,
    path::{Path, PathBuf},
};
use struct_field_names_as_array::FieldNamesAsSlice;

use tracing::{debug, warn};

/// Processes and merges all possible configuration sources.
///
/// Configuration file locations are platform-dependent.
/// To see what applies on the current platform, run `qcp --config-files`.
#[derive(Debug)]
pub struct Manager {
    /// Configuration data
    pub(super) data: Figment,
    /// The host argument this data was read for, if applicable
    pub(super) host: Option<String>,
}

impl Manager {
    /// Constructor. The structure is set up to extract data for the given `host`, if any.
    pub(super) fn new(host: Option<&str>, apply_env: bool, apply_config_files: bool) -> Self {
        let profile = if let Some(host) = host {
            figment::Profile::new(host)
        } else {
            figment::Profile::Default
        };

        let mut new1 = Self {
            data: Figment::new().select(profile),
            host: host.map(std::borrow::ToOwned::to_owned),
        };
        if apply_env {
            new1.merge_provider(ClicolorEnv {});
        }
        if apply_config_files {
            // N.B. This may leave data in a fused-error state, if a config file isn't parseable.
            new1.add_config(
                false,
                "system",
                Platform::system_config_path().as_ref(),
                host,
            );

            for p in &Platform::user_config_paths() {
                new1.add_config(true, "user", Some(p), host);
            }
        }
        new1
    }

    /// General constructor for production use
    ///
    /// Initialises this structure, reading the set of config files appropriate to the platform
    /// and the current user.
    #[must_use]
    pub fn standard(for_host: Option<&str>) -> Self {
        Self::new(for_host, true, true)
    }

    /// Testing/internal constructor, does not read files from system or apply environment; DOES apply system default.
    #[must_use]
    #[cfg(test)]
    pub(crate) fn without_files(host: Option<&str>) -> Self {
        let mut new1 = Self::new(host, false, false);
        new1.apply_system_default();
        new1
    }

    /// Testing/internal constructor, does not read files from system, apply environment, or apply system default
    #[must_use]
    #[cfg(test)]
    pub(crate) fn without_default(host: Option<&str>) -> Self {
        Self::new(host, false, false)
    }

    /// Accessor (only used in tests at the moment)
    #[cfg(test)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn host(&self) -> Option<String> {
        self.host.clone()
    }

    fn add_config(
        &mut self,
        is_user: bool,
        what: &str,
        path: Option<&PathBuf>,
        for_host: Option<&str>,
    ) {
        let Some(path) = path else {
            warn!("could not determine {what} configuration file path");
            return;
        };
        if !path.exists() {
            debug!("{what} configuration file {path:?} not present");
            return;
        }
        self.merge_ssh_config(path, for_host, is_user);
    }

    /// Returns the list of configuration files we read.
    ///
    /// This is a function of platform and the current user id.
    #[must_use]
    pub fn config_files() -> Vec<String> {
        let mut inputs = Vec::new();
        if let Some(p) = Platform::system_config_path() {
            inputs.push(p);
        }
        inputs.extend_from_slice(&Platform::user_config_paths());
        inputs
            .iter()
            .map(|p| p.as_os_str().to_string_lossy().to_string())
            .collect()
    }

    /// Merges in a data set, which is some sort of [figment::Provider](https://docs.rs/figment/latest/figment/trait.Provider.html).
    /// This uses figment's `merge` operation, which prefers to _replace_ existing items.
    ///
    /// Within qcp, we use [crate::util::derive_deftly_template_Optionalify] to implement Provider for [Configuration].
    pub fn merge_provider<T>(&mut self, provider: T)
    where
        T: Provider,
    {
        let f = std::mem::take(&mut self.data);
        self.data = f.merge(provider); // in the error case, this leaves the provider in a fused state
    }

    /// Merges in a data set from an ssh config file.
    ///
    /// The caller is expected to specify the destination host.
    /// This simplifies parsing dramatically, as it means we can apply host wildcard matching immediately.
    pub fn merge_ssh_config<F>(&mut self, file: F, host: Option<&str>, is_user: bool)
    where
        F: AsRef<Path>,
    {
        let path = file.as_ref();
        let p = super::ssh::Parser::for_path(file.as_ref(), is_user)
            .and_then(|p| p.parse_file_for(host))
            .map(|hc| self.merge_provider(hc.as_figment()));
        if let Err(e) = p {
            warn!("parsing {ff}: {e:?}", ff = path.to_string_lossy());
        }
    }

    /// Applies the system default settings, at a lower priority than everything else
    pub fn apply_system_default(&mut self) {
        let f = std::mem::take(&mut self.data);
        self.data = f.join(super::SystemDefault {});
    }

    /// Attempts to extract a particular struct from the data.
    ///
    /// Within qcp, `T` is usually [Configuration], but it isn't intrinsically required to be.
    /// (This is useful for unit testing.)
    pub(crate) fn get<'de, T>(&self) -> anyhow::Result<T, ConfigFileError>
    where
        T: Deserialize<'de>,
    {
        self.data
            .extract_lossy::<T>()
            .map_err(ConfigFileError::from)
    }

    /// Attempts to extract a single field from the data.
    ///
    /// Possible outcomes:
    /// - The field is present and has the correct type. Returns the value.
    /// - The field is present but has the wrong type. Returns InvalidType.
    /// - The field is not present. If `allow_missing` is given, that value is returned; otherwise, returns MissingField.
    /// - The field name is not valid. Returns UnknownField.
    pub(crate) fn get_config_field<'de, T>(
        &self,
        field: &str,
        default: Option<T>,
    ) -> anyhow::Result<T, ConfigFileError>
    where
        T: Deserialize<'de>,
    {
        if !Configuration::FIELD_NAMES_AS_SLICE.contains(&field) {
            return Err(ConfigFileError::from(figment::Error::unknown_field(
                field,
                Configuration::FIELD_NAMES_AS_SLICE,
            )));
        }
        if !self.data.contains(field) {
            if let Some(d) = default {
                return Ok(d);
            }
            // we need a static string for the error message, *sigh*
            let f = Configuration::FIELD_NAMES_AS_SLICE
                .iter()
                .find(|s| **s == field)
                .unwrap();
            return Err(ConfigFileError::from(figment::Error::missing_field(f)));
        }
        self.data
            .extract_inner_lossy::<T>(field)
            .map_err(ConfigFileError::from)
    }

    /// Syntactic sugar for extracting a field of type [`ColourMode`].
    /// See [`get_config`] for details.
    pub(crate) fn get_color(
        &self,
        default: Option<DeserializableEnum<ColourMode>>,
    ) -> anyhow::Result<ColourMode, ConfigFileError> {
        self.get_config_field::<DeserializableEnum<ColourMode>>("color", default)
            .map(|c| *c)
    }

    /// Performs additional validation checks on the fields present in the configuration, as far as possible.
    /// This is only useful when the [`Manager`] holds a [`Configuration`].
    pub fn validate_configuration(&self) -> Result<()> {
        let working: Configuration_Optional = self.get::<Configuration_Optional>()?;
        working.validate()?;
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::config::{Configuration, Configuration_Optional, Manager};
    use crate::util::littertray::LitterTray;
    use crate::util::{PortRange, TimeFormat};
    use engineering_repr::EngineeringQuantity;
    use serde::Deserialize;

    #[test]
    fn defaults() {
        let mgr = Manager::without_files(None);
        let result = mgr.get().unwrap();
        let expected = Configuration::system_default();
        assert_eq!(*expected, result);
    }

    #[test]
    fn config_merge() {
        // simulate a CLI
        let entered = Configuration_Optional {
            rx: Some(12345u64.into()),
            ..Default::default()
        };
        let expected = Configuration {
            rx: 12345u64.into(),
            ..Configuration::system_default().clone()
        };

        let mut mgr = Manager::without_files(None);
        mgr.merge_provider(entered);
        let result = mgr.get().unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    fn type_error() {
        // This is a semi unit test; this has a secondary goal of outputting something sensible

        #[derive(Deserialize)]
        struct Test {
            magic: i32,
        }
        LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
            rx true # invalid
            rtt 3.14159 # also invalid
            magic 42
        ",
            )?;
            let mut mgr = Manager::without_files(None);
            mgr.merge_ssh_config(path, None, false);
            // This file successfully merges into the config, but you can't extract the struct.
            let err = mgr.get::<Configuration>().unwrap_err();
            println!("Error: {err}");

            // But the config as a whole is not broken and other things can be extracted:
            let other_struct = mgr.get::<Test>().unwrap();
            assert_eq!(other_struct.magic, 42);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn field_parse_failure() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Test {
            p: PortRange,
        }

        LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
            p 234-123
        ",
            )?;
            let mut mgr = Manager::without_files(None);
            mgr.merge_ssh_config(path, None, true);
            let result = mgr.get::<Test>().unwrap_err();
            println!("{result}");
            assert!(result.to_string().contains("must be increasing"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn ssh_style() {
        #[derive(Debug, Deserialize)]
        struct Test {
            ssh_options: Vec<String>,
        }
        LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
           host bar
           ssh_options d e f
           host *
           ssh_options a b c
        ",
            )?;
            // Bear in mind: in an ssh style config file, the first match for a particular keyword wins.
            let mut mgr = Manager::without_files(Some("foo"));
            mgr.merge_ssh_config(path, Some("foo"), false);
            //println!("{}", mgr.to_display_adapter::<Configuration>(false));
            let result = mgr.get::<Test>().unwrap();
            assert_eq!(result.ssh_options, vec!["a", "b", "c"]);

            let mut mgr = Manager::without_files(Some("bar"));
            mgr.merge_ssh_config(path, Some("bar"), false);
            let result = mgr.get::<Test>().unwrap();
            assert_eq!(result.ssh_options, vec!["d", "e", "f"]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn types() {
        use crate::protocol::control::{
            CongestionController, CongestionControllerSerializingAsString,
        };

        #[derive(Debug, Deserialize, PartialEq)]
        struct Test {
            vecs: Vec<String>,
            s: String,
            i: u32,
            b: bool,
            en: CongestionControllerSerializingAsString,
            pr: PortRange,
        }

        LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
           vecs a b c
           s foo
           i 42
           b true
           en bbr
           pr 123-456
        ",
            );
            let mut mgr = Manager::without_files(Some("foo"));
            mgr.merge_ssh_config(path, Some("foo"), false);
            // println!("{mgr}");
            let result = mgr.get::<Test>().unwrap();
            assert_eq!(
                result,
                Test {
                    vecs: vec!["a".into(), "b".into(), "c".into()],
                    s: "foo".into(),
                    i: 42,
                    b: true,
                    en: CongestionController::Bbr.into(),
                    pr: PortRange {
                        begin: 123,
                        end: 456
                    }
                }
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn bools() {
        #[derive(Debug, Deserialize)]
        struct Test {
            b: bool,
        }

        LitterTray::try_with(|tray| {
            let path = "testfile";

            for (s, expected) in [
                ("yes", true),
                ("true", true),
                ("1", true),
                ("no", false),
                ("false", false),
                ("0", false),
            ] {
                let _ = tray
                    .create_text(path, &format!("b {s}"))
                    .expect("Unable to write tempfile");
                // ... test it
                let mut mgr = Manager::without_files(Some("foo"));
                mgr.merge_ssh_config(path, Some("foo"), false);
                let result = mgr
                    .get::<Test>()
                    .inspect_err(|e| println!("ERROR: {e}"))
                    .unwrap();
                assert_eq!(result.b, expected);
            }
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn invalid_data() {
        use crate::protocol::control::CongestionControllerSerializingAsString;

        #[derive(Debug, Deserialize, PartialEq)]
        struct Test {
            b: bool,
            en: CongestionControllerSerializingAsString,
            i: u32,
            pr: PortRange,
        }
        LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
           i wombat
           b wombat
           en wombat
           pr wombat
        ",
            );
            let mut mgr = Manager::new(None, false, false);
            mgr.merge_ssh_config(path, Some("foo"), false);
            //println!("{mgr:?}");
            let err = mgr.get::<Test>().unwrap_err();
            println!("{err}");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn cli_beats_config_file() {
        let _ee = EngineeringQuantity::<u32>::from_raw(1, 2);
        // simulate a CLI
        let entered = Configuration_Optional {
            rx: Some(12345u64.into()),
            ..Default::default()
        };
        LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
            Host foo
            rx 66666
        ",
            );

            let mut mgr = Manager::without_files(Some("foo"));
            mgr.merge_ssh_config(path, Some("foo"), false);
            // The order of merging mirrors what happens in Manager::try_from(&CliArgs)
            mgr.merge_provider(entered);
            assert_eq!(mgr.host(), Some("foo".to_string()));
            let result = mgr.get::<Configuration>().unwrap();
            assert_eq!(12345, result.rx());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn parse_eng_quantity() {
        LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
            Host foo
            rx 10M5
        ",
            )?;
            let mut mgr = Manager::without_files(Some("foo"));
            mgr.merge_ssh_config(path, Some("foo"), false);
            //println!("{mgr:?}");
            let result = mgr.get::<Configuration>().unwrap();
            assert_eq!(10_500_000, result.rx());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn invalid_enum() {
        LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
           color wombat
        ",
            );
            let mut mgr = Manager::new(None, false, false);
            mgr.merge_ssh_config(path, Some("foo"), false);
            //println!("{mgr:?}");
            let err = mgr.get::<Configuration_Optional>().unwrap_err();
            println!("{err}");
            assert!(err.to_string().contains("expected one of"));
            assert!(err.to_string().contains("auto"));
            assert!(err.to_string().contains("on"));
            assert!(err.to_string().contains("always"));
            assert!(err.to_string().contains("color"));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn field_extraction() {
        let mgr = Manager::without_files(None);
        let def = Configuration::system_default();
        assert_eq!(
            u64::from(
                mgr.get_config_field::<EngineeringQuantity<u64>>("rx", None)
                    .unwrap()
            ),
            def.rx()
        );
        assert_eq!(
            mgr.get_config_field::<TimeFormat>("time_format", None),
            Ok(def.time_format)
        );
        assert_eq!(mgr.get_color(None), Ok(*def.color));

        // wrong type
        assert!(
            mgr.get_config_field::<bool>("rx", None)
                .unwrap_err()
                .to_string()
                .contains("expected a boolean")
        );
        // typo'd name
        assert!(
            mgr.get_config_field::<u64>("no such field", None)
                .unwrap_err()
                .to_string()
                .contains("no such field")
        );
        // field not present
        let empty_mgr = Manager::new(None, false, false);
        assert!(
            empty_mgr
                .get_config_field::<u64>("rx", None)
                .unwrap_err()
                .to_string()
                .contains("missing field")
        );
    }
}
