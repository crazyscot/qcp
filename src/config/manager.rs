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
    fmt::{Debug, Display},
    path::{Path, PathBuf},
};
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

/// Processes and merges all possible configuration sources
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
    /// Initialises this structure with the standard set of OS-specific file paths
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

    /// Returns a list of configuration files
    ///
    /// This is a function of platform and the current user id
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

    /// Merges in a data set.
    ///
    /// `T` is expected to be a type created by [crate::util::derive_deftly_template_Optionalify].
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
    /// Type `T` may be `Configuration` or one of its sub-elements.
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
    fn new(field: &str, value: Value, meta: Option<&Metadata>) -> Self {
        let value = match value {
            Value::String(_tag, s) => s,
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
            // we don't currently support dict or array types
            Value::Dict(_tag, _btree_map) => todo!(),
            Value::Array(_tag, _vec) => todo!(),
        };

        let source = if let Some(m) = meta {
            m.source
                .as_ref()
                .map_or_else(|| m.name.to_string(), figment::Source::to_string)
        } else {
            String::new()
        };

        Self {
            field: field.into(),
            value,
            source,
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
            fields.push(PrettyConfig::new(field, value, meta));
        }
        write!(f, "{}", Table::new(fields).with(Style::sharp()))
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use serde::Deserialize;
    use tempfile::TempDir;

    use crate::transport::{BandwidthParams, BandwidthParams_Optional};

    use super::{Configuration, Manager};

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
        let entered = BandwidthParams_Optional {
            rx: Some(12345.into()),
            ..Default::default()
        };
        let expected = Configuration {
            bandwidth: BandwidthParams {
                rx: 12345.into(),
                ..Default::default()
            },
        };

        let mut mgr = Manager::without_files();
        mgr.merge_provider(entered);
        let result = mgr.get().unwrap();
        assert_eq!(expected, result);
    }

    #[test]
    fn extract_substruct() {
        let cfg: BandwidthParams = Manager::without_files().get().unwrap();
        assert_eq!(cfg, BandwidthParams::default());
    }

    fn make_tempfile(data: &str, filename: &str) -> (PathBuf, TempDir) {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(filename);
        std::fs::write(&path, data).expect("Unable to write tempfile");
        // println!("temp file is {:?}", &path);
        (path, tempdir)
    }

    #[test]
    fn dump_config_cli_and_toml() {
        // Not a unit test as such; this is a human test
        let (path, _tempdir) = make_tempfile(
            r#"
            tx = 42
            congestion = "Bbr"
            unused__ = 42
        "#,
            "test.toml",
        );
        let fake_cli = BandwidthParams_Optional {
            rtt: Some(999),
            initial_congestion_window: Some(Some(67890)), // yeah the double-Some is a bit of a wart
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
        let (path, _tempdir) = make_tempfile(
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

        let (path, _tempdir) = make_tempfile(
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
}
