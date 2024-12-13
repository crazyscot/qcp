//! Configuration file wrangling
// (c) 2024 Ross Younger

use super::Configuration;

use anyhow::Result;
use figment::{
    providers::{Format, Serialized, Toml},
    value::Value,
    Figment, Metadata, Provider,
};
use serde::Deserialize;
use std::{
    collections::HashSet,
    fmt::{Debug, Display},
    path::{Path, PathBuf},
};
use struct_field_names_as_array::FieldNamesAsSlice;
use tabled::{settings::style::Style, Table, Tabled};

use tracing::{trace, warn};

// PATHS /////////////////////////////////////////////////////////////////////////////////////////////////////

const BASE_CONFIG_FILENAME: &str = "qcp.toml";

#[cfg(unix)]
fn user_config_dir() -> Result<PathBuf> {
    // home directory for now
    use etcetera::BaseStrategy as _;
    Ok(etcetera::choose_base_strategy()?.home_dir().into())
}

#[cfg(windows)]
fn user_config_dir() -> Result<PathBuf> {
    use etcetera::{choose_app_strategy, AppStrategy as _, AppStrategyArgs};

    Ok(choose_app_strategy(AppStrategyArgs {
        top_level_domain: "com".to_string(),
        author: "TeamQCP".to_string(),
        app_name: env!("CARGO_PKG_NAME").to_string(),
    })?
    .config_dir())
}

#[cfg(unix)]
fn user_config_path() -> Result<PathBuf> {
    // ~/.<filename> for now
    let mut d: PathBuf = user_config_dir()?;
    d.push(format!(".{BASE_CONFIG_FILENAME}"));
    Ok(d)
}

#[cfg(unix)]
fn system_config_path() -> PathBuf {
    // /etc/<filename> for now
    let mut p: PathBuf = PathBuf::new();
    p.push("/etc");
    p.push(BASE_CONFIG_FILENAME);
    p
}

// SYSTEM DEFAULTS //////////////////////////////////////////////////////////////////////////////////////////////

/// A `[https://docs.rs/figment/latest/figment/trait.Provider.html](figment::Provider)` that holds
/// our set of fixed system default options
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
}

fn add_user_config(f: Figment) -> Figment {
    let path = match user_config_path() {
        Ok(p) => p,
        Err(e) => {
            warn!("could not determine user configuration file path: {e}");
            return f;
        }
    };
    if !path.exists() {
        trace!("user configuration file {path:?} not present");
        return f;
    }
    f.merge(Toml::file(path.as_path()))
}

fn add_system_config(f: Figment) -> Figment {
    let path = system_config_path();
    if !path.exists() {
        trace!("system configuration file {path:?} not present");
        return f;
    }
    f.merge(Toml::file(path.as_path()))
}

impl Default for Manager {
    /// Initialises this structure fully-empty (for new(), or testing)
    fn default() -> Self {
        Self {
            data: Figment::default(),
        }
    }
}

impl Manager {
    /// Initialises this structure, reading the set of config files appropriate to the platform
    /// and the current user.
    #[must_use]
    pub fn new() -> Self {
        let mut data = Figment::new().merge(SystemDefault::default());
        data = add_system_config(data);

        // N.B. This may leave data in a fused-error state, if a data file isn't parseable.
        data = add_user_config(data);
        Self {
            data,
            //..Self::default()
        }
    }

    /// Returns the list of configuration files we read.
    ///
    /// This is a function of platform and the current user id.
    pub fn config_files() -> Vec<String> {
        let inputs = vec![Ok(system_config_path()), user_config_path()];

        inputs
            .into_iter()
            .filter_map(std::result::Result::ok)
            .map(|p| p.into_os_string().to_string_lossy().into())
            .collect()
    }

    /// Testing/internal constructor, does not read files from system
    #[must_use]
    #[allow(unused)]
    pub(crate) fn without_files() -> Self {
        let data = Figment::new().merge(SystemDefault::default());
        Self {
            data,
            //..Self::default()
        }
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

    /// Merges in a data set from a TOML file
    pub fn merge_toml_file<T>(&mut self, toml: T)
    where
        T: AsRef<Path>,
    {
        let path = toml.as_ref();
        let provider = Toml::file_exact(path);
        self.merge_provider(provider);
    }

    /// Attempts to extract a particular struct from the data.
    ///
    /// Within qcp, `T` is usually [Configuration], but it isn't intrinsically required to be.
    pub fn get<'de, T>(&self) -> anyhow::Result<T, figment::Error>
    where
        T: Deserialize<'de>,
    {
        self.data.extract::<T>()
    }
}

// PRETTY PRINT SUPPORT ///////////////////////////////////////////////////////////////////////////////////////

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

    fn new(field: &str, value: &Value, meta: Option<&Metadata>) -> Self {
        Self {
            field: field.into(),
            value: PrettyConfig::render_value(value),
            source: PrettyConfig::render_source(meta),
        }
    }
}

impl Display for Manager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let data = match self.data.data() {
            Ok(d) => d,
            Err(e) => {
                // This isn't terribly helpful as it doesn't have metadata attached; BUT attempting to get() a struct does.
                return write!(f, "error: {e}");
            }
        };
        let data = data.get(&figment::Profile::Default).unwrap();

        let mut fields = Vec::<PrettyConfig>::new();

        for field in data.keys() {
            let value = self.data.find_value(field);
            let value = match value {
                Ok(v) => v,
                Err(e) => {
                    writeln!(f, "error on field {field}: {e}")?;
                    continue;
                }
            };
            let meta = self.data.find_metadata(field);
            fields.push(PrettyConfig::new(field, &value, meta));
        }
        write!(f, "{}", Table::new(fields).with(Style::sharp()))
    }
}

/// Pretty-printing type wrapper to Manager
#[derive(Debug)]
pub struct DisplayAdapter<'a> {
    /// Data source
    source: &'a Manager,
    /// Whether to warn if unused fields are present
    warn_on_unused: bool,
    /// The fields we want to output
    fields: HashSet<String>,
}

impl Manager {
    /// Creates a `DisplayAdapter` for this struct with the given options.
    ///
    /// # Returns
    /// An ephemeral structure implementing `Display`.
    #[must_use]
    pub fn to_display_adapter<'de, T>(&self, warn_on_unused: bool) -> DisplayAdapter<'_>
    where
        T: Deserialize<'de> + FieldNamesAsSlice,
    {
        let mut fields = HashSet::<String>::new();
        fields.extend(T::FIELD_NAMES_AS_SLICE.iter().map(|s| String::from(*s)));
        DisplayAdapter {
            source: self,
            warn_on_unused,
            fields,
        }
    }
}

impl Display for DisplayAdapter<'_> {
    /// Formats the contents of this structure which are relevant to a given output type.
    ///
    /// N.B. This function uses CLI styling.
    #[allow(clippy::missing_panics_doc)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use crate::cli::styles::{ERROR_S, WARNING_S};
        use anstream::eprintln;
        use owo_colors::OwoColorize as _;

        let data = match self.source.data.data() {
            Ok(d) => d,
            Err(e) => {
                // This isn't terribly helpful as it doesn't have metadata attached; BUT attempting to get() a struct does.
                eprintln!("{} {e}", "ERROR".style(*ERROR_S));
                return Ok(());
            }
        };
        // panic is impossible on the Default profile, hence #[allow(clippy::missing_panics_doc)]
        let data = data.get(&figment::Profile::Default).unwrap();

        let mut output = Vec::<PrettyConfig>::new();

        for field in data.keys() {
            let meta = self.source.data.find_metadata(field);
            if self.fields.contains(field) {
                let value = self.source.data.find_value(field);
                let value = match value {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{}: error on {field}: {e}", "WARNING".style(*WARNING_S));
                        continue;
                    }
                };
                output.push(PrettyConfig::new(field, &value, meta));
            } else if self.warn_on_unused {
                let source = PrettyConfig::render_source(meta);
                eprintln!(
                    "{}: unrecognised field `{field}` in {source}",
                    "WARNING".style(*WARNING_S)
                );
            }
        }
        write!(f, "{}", Table::new(output).with(Style::sharp()))
    }
}

#[cfg(test)]
mod test {
    use crate::config::{Configuration, Configuration_Optional, Manager};
    use crate::util::{make_test_tempfile, PortRange};
    use serde::Deserialize;

    #[test]
    fn defaults() {
        let mgr = Manager::without_files();
        let result = mgr.get().unwrap();
        let expected = Configuration::default();
        assert_eq!(expected, result);
    }

    #[test]
    fn config_merge() {
        // simulate a CLI
        let entered = Configuration_Optional {
            rx: Some(12345.into()),
            ..Default::default()
        };
        let expected = Configuration {
            rx: 12345.into(),
            ..Default::default()
        };

        let mut mgr = Manager::without_files();
        mgr.merge_provider(entered);
        let result = mgr.get().unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    fn dump_config_cli_and_toml() {
        // Not a unit test as such; this is a human test
        let (path, _tempdir) = make_test_tempfile(
            r#"
            tx = 42
            congestion = "Bbr"
            unused__ = 42
        "#,
            "test.toml",
        );
        let fake_cli = Configuration_Optional {
            rtt: Some(999),
            initial_congestion_window: Some(67890),
            ..Default::default()
        };
        let mut mgr = Manager::without_files();
        mgr.merge_toml_file(path);
        mgr.merge_provider(fake_cli);
        println!("{mgr}");
    }

    #[test]
    fn unparseable_toml() {
        // This is a semi unit test; there is one assert, but the secondary goal is that it outputs something sensible
        let (path, _tempdir) = make_test_tempfile(
            r"
            a = 1
            rx 123 # this line is a syntax error
            b = 2
        ",
            "test.toml",
        );
        let mut mgr = Manager::without_files();
        mgr.merge_toml_file(path);
        let get = mgr.get::<Configuration>();
        assert!(get.is_err());
        println!("{}", get.unwrap_err());
        // println!("{mgr}");
    }

    #[test]
    fn type_error() {
        // This is a semi unit test; this has a secondary goal of outputting something sensible

        #[derive(Deserialize)]
        struct Test {
            magic_: i32,
        }

        let (path, _tempdir) = make_test_tempfile(
            r"
            rx = true # invalid
            rtt = 3.14159 # also invalid
            magic_ = 42
        ",
            "test.toml",
        );
        let mut mgr = Manager::without_files();
        mgr.merge_toml_file(path);
        // This TOML successfully merges into the config, but you can't extract the struct.
        let err = mgr.get::<Configuration>().unwrap_err();
        println!("Error: {err}");
        // TODO: Would really like a rich error message here pointing to the failing key and errant file.
        // We get no metadata in the error :-(

        // But the config as a whole is not broken and other things can be extracted:
        let other_struct = mgr.get::<Test>().unwrap();
        assert_eq!(other_struct.magic_, 42);
    }

    #[test]
    fn int_or_string() {
        #[derive(Deserialize)]
        struct Test {
            t1: PortRange,
            t2: PortRange,
            t3: PortRange,
        }
        let (path, _tempdir) = make_test_tempfile(
            r#"
            t1 = 1234
            t2 = "2345"
            t3 = "123-456"
        "#,
            "test.toml",
        );
        let mut mgr = Manager::without_files();
        mgr.merge_toml_file(path);
        let res = mgr.get::<Test>().unwrap();
        assert_eq!(
            res.t1,
            PortRange {
                begin: 1234,
                end: 1234
            }
        );
        assert_eq!(
            res.t2,
            PortRange {
                begin: 2345,
                end: 2345
            }
        );
        assert_eq!(
            res.t3,
            PortRange {
                begin: 123,
                end: 456
            }
        );
    }

    #[test]
    fn array_type() {
        #[derive(Deserialize)]
        struct Test {
            ii: Vec<i32>,
        }

        let (path, _tempdir) = make_test_tempfile(
            r"
            ii = [1,2,3,4,6]
        ",
            "test.toml",
        );
        let mut mgr = Manager::without_files();
        mgr.merge_toml_file(path);
        let result = mgr.get::<Test>().unwrap();
        assert_eq!(result.ii, vec![1, 2, 3, 4, 6]);
    }

    #[test]
    fn field_parse_failure() {
        #[derive(Debug, Deserialize)]
        struct Test {
            _p: PortRange,
        }

        let (path, _tempdir) = make_test_tempfile(
            r#"
            _p = "234-123"
        "#,
            "test.toml",
        );
        let mut mgr = Manager::without_files();
        mgr.merge_toml_file(path);
        let result = mgr.get::<Test>().unwrap_err();
        println!("{result}");
        assert!(result
            .to_string()
            .contains("invalid port range \"234-123\""));
    }
}
