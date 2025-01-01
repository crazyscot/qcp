//! Configuration file wrangling
// (c) 2024 Ross Younger

use crate::os::{AbstractPlatform as _, Platform};

use super::{ssh::SshConfigError, Configuration};

use figment::{providers::Serialized, value::Value, Figment, Metadata, Provider};
use heck::ToUpperCamelCase;
use serde::Deserialize;
use std::{
    collections::HashSet,
    fmt::{Debug, Display},
    path::{Path, PathBuf},
};
use struct_field_names_as_array::FieldNamesAsSlice;
use tabled::{
    settings::{object::Rows, style::Style, Color},
    Table, Tabled,
};

use tracing::{debug, warn};

// SYSTEM DEFAULTS //////////////////////////////////////////////////////////////////////////////////////////////

/// A [`figment::Provider`](https://docs.rs/figment/latest/figment/trait.Provider.html) that holds
/// the set of system default options
#[derive(Default)]
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
        Serialized::defaults(Configuration::default()).data()
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

impl Default for Manager {
    /// Initialises this structure fully-empty (for new(), or testing)
    fn default() -> Self {
        Self {
            data: Figment::default(),
            host: None,
        }
    }
}

impl Manager {
    /// Initialises this structure, reading the set of config files appropriate to the platform
    /// and the current user.
    #[must_use]
    pub fn standard(for_host: Option<&str>) -> Self {
        let mut new1 = Self {
            data: Figment::new(),
            host: for_host.map(std::borrow::ToOwned::to_owned),
        };
        new1.merge_provider(SystemDefault::default());
        // N.B. This may leave data in a fused-error state, if a config file isn't parseable.
        new1.add_config(false, "system", Platform::system_config_path(), for_host);
        new1.add_config(true, "user", Platform::user_config_path(), for_host);
        new1
    }

    /// Accessor (only used in tests at the moment)
    #[cfg(test)]
    fn host(&self) -> Option<String> {
        self.host.clone()
    }

    fn add_config(
        &mut self,
        is_user: bool,
        what: &str,
        path: Option<PathBuf>,
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
        let inputs = vec![Platform::system_config_path(), Platform::user_config_path()];

        inputs
            .into_iter()
            .filter_map(|p| Some(p?.into_os_string().to_string_lossy().to_string()))
            .collect()
    }

    /// Testing/internal constructor, does not read files from system
    #[must_use]
    #[cfg(test)]
    pub(crate) fn without_files(host: Option<&str>) -> Self {
        let data = Figment::new().merge(SystemDefault::default());
        let host = host.map(std::string::ToString::to_string);
        Self { data, host }
    }

    /// Merges in a data set, which is some sort of [figment::Provider](https://docs.rs/figment/latest/figment/trait.Provider.html).
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
            warn!("parsing {ff}: {e}", ff = path.to_string_lossy());
        }
    }

    /// Attempts to extract a particular struct from the data.
    ///
    /// Within qcp, `T` is usually [Configuration], but it isn't intrinsically required to be.
    /// (This is useful for unit testing.)
    pub(crate) fn get<'de, T>(&self) -> anyhow::Result<T, SshConfigError>
    where
        T: Deserialize<'de>,
    {
        let profile = if let Some(host) = &self.host {
            figment::Profile::new(host)
        } else {
            figment::Profile::Default
        };

        self.data
            .clone()
            .select(profile)
            .extract_lossy::<T>()
            .map_err(SshConfigError::from)
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
        let mut data = self.source.data.clone();

        let mut output = Vec::<PrettyConfig>::new();
        // First line of the table is special
        let (host_string, host_colour) = if let Some(host) = &self.source.host {
            let profile = figment::Profile::new(host);
            data = data.select(profile);
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
        write!(
            f,
            "{}",
            Table::new(output)
                .modify(Rows::single(1), host_colour)
                .with(Style::sharp())
        )
    }
}

#[cfg(test)]
mod test {
    use crate::config::ssh::SshConfigError;
    use crate::config::{Configuration, Configuration_Optional, Manager};
    use crate::util::{make_test_tempfile, PortRange};
    use engineering_repr::EngineeringQuantity;
    use serde::Deserialize;

    #[test]
    fn defaults() {
        let mgr = Manager::without_files(None);
        let result = mgr.get().unwrap();
        let expected = Configuration::default();
        assert_eq!(expected, result);
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
            ..Default::default()
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
        use crate::transport::CongestionControllerType;

        #[derive(Debug, Deserialize, PartialEq)]
        struct Test {
            vecs: Vec<String>,
            s: String,
            i: u32,
            b: bool,
            en: CongestionControllerType,
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
                en: CongestionControllerType::Bbr,
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
        use crate::transport::CongestionControllerType;

        #[derive(Debug, Deserialize, PartialEq)]
        struct Test {
            b: bool,
            en: CongestionControllerType,
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
        let mut mgr = Manager::default();
        mgr.merge_ssh_config(&path, Some("foo"), false);
        //println!("{mgr:?}");
        let err = mgr.get::<Test>().map_err(SshConfigError::from).unwrap_err();
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
}
