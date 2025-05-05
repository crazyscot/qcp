//! Configuration file wrangling
// (c) 2024 Ross Younger

use crate::{
    cli::styles::use_colours,
    os::{AbstractPlatform as _, Platform},
};

use super::{Configuration, Configuration_Optional, ssh::ConfigFileError};

use anyhow::Result;
use figment::{Figment, Metadata, Provider, providers::Serialized, value::Value};
use heck::ToUpperCamelCase;
use serde::Deserialize;
use std::{
    collections::HashSet,
    fmt::{Debug, Display},
    path::{Path, PathBuf},
};
use struct_field_names_as_array::FieldNamesAsSlice;
use tabled::{
    Table, Tabled,
    settings::{Color, object::Rows, style::Style},
};

use tracing::{debug, warn};

// SYSTEM DEFAULTS //////////////////////////////////////////////////////////////////////////////////////////////

/// A [`figment::Provider`](https://docs.rs/figment/latest/figment/trait.Provider.html) that holds
/// the set of system default options
struct SystemDefault {}

impl SystemDefault {
    const META_NAME: &str = "default";
}

impl Provider for SystemDefault {
    fn metadata(&self) -> Metadata {
        figment::Metadata::named(Self::META_NAME)
    }

    fn data(
        &self,
    ) -> std::result::Result<
        figment::value::Map<figment::Profile, figment::value::Dict>,
        figment::Error,
    > {
        Serialized::defaults(Configuration::system_default()).data()
    }
}

// CLICOLOR SPECIAL HANDLING //////////////////////////////////////////////////////////////////////////////////

struct ClicolorEnv {}
impl ClicolorEnv {
    const META_NAME: &str = "environment variable(s)";
}

impl Provider for ClicolorEnv {
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

// CONFIG MANAGER /////////////////////////////////////////////////////////////////////////////////////////////

/// Processes and merges all possible configuration sources.
///
/// Configuration file locations are platform-dependent.
/// To see what applies on the current platform, run `qcp --config-files`.
#[derive(Debug)]
pub struct Manager {
    /// Configuration data
    data: Figment,
    /// The host argument this data was read for, if applicable
    host: Option<String>,
}

impl Manager {
    /// Constructor. The structure is set up to extract data for the given `host`, if any.
    fn new(host: Option<&str>, apply_env: bool, apply_config_files: bool) -> Self {
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
        self.data = f.join(SystemDefault {});
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

    /// Performs additional validation checks on the fields present in the configuration, as far as possible.
    /// This is only useful when the [`Manager`] holds a [`Configuration`].
    pub fn validate_configuration(&self) -> Result<()> {
        let working: Configuration_Optional = self.get::<Configuration_Optional>()?;
        working.validate()?;
        Ok(())
    }
}

// PRETTY PRINT SUPPORT ///////////////////////////////////////////////////////////////////////////////////////

/// Data type used when rendering the config table
#[derive(Tabled)]
struct PrettyConfig {
    field: String,
    value: String,
    source: String,
}

impl PrettyConfig {
    fn render_source(meta: Option<&Metadata>) -> String {
        if let Some(m) = meta {
            m.source
                .as_ref()
                .map_or_else(|| m.name.to_string(), figment::Source::to_string)
        } else {
            String::new()
        }
    }

    fn render_value(value: &Value) -> String {
        match value {
            Value::String(_tag, s) => s.to_string(),
            Value::Char(_tag, c) => c.to_string(),
            Value::Bool(_tag, b) => b.to_string(),
            Value::Num(_tag, num) => {
                if let Some(i) = num.to_i128() {
                    i.to_string()
                } else if let Some(u) = num.to_u128() {
                    u.to_string()
                } else if let Some(ff) = num.to_f64() {
                    ff.to_string()
                } else {
                    todo!("unhandled Num case");
                }
            }
            Value::Empty(_tag, _) => "<empty>".into(),
            // we don't currently support dict types
            Value::Dict(_tag, _dict) => todo!(),
            Value::Array(_tag, vec) => {
                format!(
                    "[{}]",
                    vec.iter()
                        .map(PrettyConfig::render_value)
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }
        }
    }

    fn new<F: Into<String>>(field: F, value: &Value, meta: Option<&Metadata>) -> Self {
        Self {
            field: field.into(),
            value: PrettyConfig::render_value(value),
            source: PrettyConfig::render_source(meta),
        }
    }
}

/// Pretty-printing type wrapper to Manager
#[derive(Debug)]
pub struct DisplayAdapter<'a> {
    /// Data source
    source: &'a Manager,
    /// The fields we want to output. (If empty, outputs everything.)
    fields: HashSet<String>,
}

impl Manager {
    /// Creates a `DisplayAdapter` for this struct with the given options.
    ///
    /// # Returns
    /// An ephemeral structure implementing `Display`.
    #[must_use]
    pub fn to_display_adapter<'de, T>(&self) -> DisplayAdapter<'_>
    where
        T: Deserialize<'de> + FieldNamesAsSlice,
    {
        let mut fields = HashSet::<String>::new();
        fields.extend(T::FIELD_NAMES_AS_SLICE.iter().map(|s| String::from(*s)));
        DisplayAdapter {
            source: self,
            fields,
        }
    }
}

impl Display for DisplayAdapter<'_> {
    /// Formats the contents of this structure which are relevant to a given output type.
    ///
    /// N.B. This function uses CLI styling.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = &self.source.data;

        let mut output = Vec::<PrettyConfig>::new();
        // First line of the table is special
        let (host_string, host_colour) = if let Some(host) = &self.source.host {
            (host.clone(), Color::FG_GREEN)
        } else {
            ("* (globals)".into(), Color::FG_CYAN)
        };
        output.push(PrettyConfig {
            field: "(Remote host)".into(),
            value: host_string,
            source: String::new(),
        });

        let mut keys = self.fields.iter().collect::<Vec<_>>();
        keys.sort();

        for field in keys {
            if let Ok(value) = data.find_value(field) {
                let meta = data.get_metadata(value.tag());
                output.push(PrettyConfig::new(field.to_upper_camel_case(), &value, meta));
            }
        }
        let mut writable = Table::new(output);
        let _ = writable.with(Style::sharp());
        if use_colours() {
            let _ = writable.modify(Rows::single(1), host_colour);
        }
        write!(f, "{writable}")
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::config::{Configuration, Configuration_Optional, Manager};
    use crate::util::{PortRange, make_test_tempfile};
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

        let (path, _tempdir) = make_test_tempfile(
            r"
            rx true # invalid
            rtt 3.14159 # also invalid
            magic 42
        ",
            "test.conf",
        );
        let mut mgr = Manager::without_files(None);
        mgr.merge_ssh_config(path, None, false);
        // This file successfully merges into the config, but you can't extract the struct.
        let err = mgr.get::<Configuration>().unwrap_err();
        println!("Error: {err}");

        // But the config as a whole is not broken and other things can be extracted:
        let other_struct = mgr.get::<Test>().unwrap();
        assert_eq!(other_struct.magic, 42);
    }

    #[test]
    fn field_parse_failure() {
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Test {
            p: PortRange,
        }

        let (path, _tempdir) = make_test_tempfile(
            r"
            p 234-123
        ",
            "test.conf",
        );
        let mut mgr = Manager::without_files(None);
        mgr.merge_ssh_config(path, None, true);
        let result = mgr.get::<Test>().unwrap_err();
        println!("{result}");
        assert!(result.to_string().contains("must be increasing"));
    }

    #[test]
    fn ssh_style() {
        #[derive(Debug, Deserialize)]
        struct Test {
            ssh_options: Vec<String>,
        }
        // Bear in mind: in an ssh style config file, the first match for a particular keyword wins.
        let (path, _tempdir) = make_test_tempfile(
            r"
           host bar
           ssh_options d e f
           host *
           ssh_options a b c
        ",
            "test.conf",
        );
        let mut mgr = Manager::without_files(Some("foo"));
        mgr.merge_ssh_config(&path, Some("foo"), false);
        //println!("{}", mgr.to_display_adapter::<Configuration>(false));
        let result = mgr.get::<Test>().unwrap();
        assert_eq!(result.ssh_options, vec!["a", "b", "c"]);

        let mut mgr = Manager::without_files(Some("bar"));
        mgr.merge_ssh_config(&path, Some("bar"), false);
        let result = mgr.get::<Test>().unwrap();
        assert_eq!(result.ssh_options, vec!["d", "e", "f"]);
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

        let (path, _tempdir) = make_test_tempfile(
            r"
           vecs a b c
           s foo
           i 42
           b true
           en bbr
           pr 123-456
        ",
            "test.conf",
        );
        let mut mgr = Manager::without_files(Some("foo"));
        mgr.merge_ssh_config(&path, Some("foo"), false);
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
    }

    #[test]
    fn bools() {
        #[derive(Debug, Deserialize)]
        struct Test {
            b: bool,
        }

        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("testfile");

        for (s, expected) in [
            ("yes", true),
            ("true", true),
            ("1", true),
            ("no", false),
            ("false", false),
            ("0", false),
        ] {
            std::fs::write(
                &path,
                format!(
                    r"
                        b {s}
                    "
                ),
            )
            .expect("Unable to write tempfile");
            // ... test it
            let mut mgr = Manager::without_files(Some("foo"));
            mgr.merge_ssh_config(&path, Some("foo"), false);
            let result = mgr
                .get::<Test>()
                .inspect_err(|e| println!("ERROR: {e}"))
                .unwrap();
            assert_eq!(result.b, expected);
        }
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

        let (path, _tempdir) = make_test_tempfile(
            r"
           i wombat
           b wombat
           en wombat
           pr wombat
        ",
            "test.conf",
        );
        let mut mgr = Manager::new(None, false, false);
        mgr.merge_ssh_config(&path, Some("foo"), false);
        //println!("{mgr:?}");
        let err = mgr.get::<Test>().unwrap_err();
        println!("{err}");
    }

    #[test]
    fn cli_beats_config_file() {
        let _ee = EngineeringQuantity::<u32>::from_raw(1, 2);
        // simulate a CLI
        let entered = Configuration_Optional {
            rx: Some(12345u64.into()),
            ..Default::default()
        };
        let (path, _tempdir) = make_test_tempfile(
            r"
            Host foo
            rx 66666
        ",
            "test.conf",
        );

        let mut mgr = Manager::without_files(Some("foo"));
        mgr.merge_ssh_config(&path, Some("foo"), false);
        // The order of merging mirrors what happens in Manager::try_from(&CliArgs)
        mgr.merge_provider(entered);
        assert_eq!(mgr.host(), Some("foo".to_string()));
        let result = mgr.get::<Configuration>().unwrap();
        assert_eq!(12345, result.rx());
    }

    #[test]
    fn parse_eng_quantity() {
        let (path, _tempdir) = make_test_tempfile(
            r"
            Host foo
            rx 10M5
        ",
            "test.conf",
        );
        let mut mgr = Manager::without_files(Some("foo"));
        mgr.merge_ssh_config(&path, Some("foo"), false);
        //println!("{mgr:?}");
        let result = mgr.get::<Configuration>().unwrap();
        assert_eq!(10_500_000, result.rx());
    }

    #[test]
    fn invalid_enum() {
        let (path, _tempdir) = make_test_tempfile(
            r"
           color wombat
        ",
            "test.conf",
        );
        let mut mgr = Manager::new(None, false, false);
        mgr.merge_ssh_config(&path, Some("foo"), false);
        //println!("{mgr:?}");
        let err = mgr.get::<Configuration_Optional>().unwrap_err();
        println!("{err}");
        assert!(err.to_string().contains("expected one of"));
        assert!(err.to_string().contains("auto"));
        assert!(err.to_string().contains("on"));
        assert!(err.to_string().contains("always"));
        assert!(err.to_string().contains("color"));
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test2 {
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
