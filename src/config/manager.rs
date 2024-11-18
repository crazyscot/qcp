//! Configuration file wrangling
// (c) 2024 Ross Younger

use super::{structure::Configuration_Optional, Configuration};

use anyhow::Result;
use figment::{
    providers::{Format, Serialized, Toml},
    Figment, Provider,
};
use std::path::PathBuf;

use tracing::{trace, warn};

const CONFIG_FILENAME: &str = "qcp.toml";

#[cfg(unix)]
fn user_config_dir() -> Result<PathBuf> {
    use etcetera::BaseStrategy as _;
    Ok(etcetera::choose_base_strategy()?.config_dir())
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

fn user_config_path() -> Result<PathBuf> {
    let mut d = user_config_dir()?;
    d.set_file_name(CONFIG_FILENAME);
    Ok(d)
}

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
    pub(crate) fn new(cli: Option<Configuration_Optional>) -> Self {
        let mut data = Figment::new().merge(Serialized::defaults(Configuration::default()));
        // TODO: systemwide config file ?
        data = add_user_config(data);
        if let Some(o) = cli {
            data = data.merge(o);
        }
        Self {
            data,
            //..Self::default()
        }
    }

    /// Testing constructor, does not read files from system
    #[must_use]
    pub(crate) fn without_files(cli: Option<Configuration_Optional>) -> Self {
        let mut data = Figment::new().merge(Serialized::defaults(Configuration::default()));
        if let Some(o) = cli {
            data = data.merge(o);
        }
        Self {
            data,
            //..Self::default()
        }
    }

    pub(crate) fn merge<T>(&mut self, provider: T)
    where
        T: Provider,
    {
        let f = std::mem::take(&mut self.data);
        self.data = f.merge(provider);
    }

    pub(crate) fn get(&self) -> anyhow::Result<Configuration> {
        Ok(self.data.extract()?)
    }
}

#[cfg(test)]
mod test {
    use super::{Configuration, Configuration_Optional, Manager};

    #[test]
    fn config_merge() {
        // simulate a CLI
        let cli = Configuration_Optional {
            rx: Some(12345.into()),
            ..Default::default()
        };
        let expected = Configuration {
            rx: 12345.into(),
            ..Default::default()
        };

        let mgr = Manager::without_files(Some(cli));
        let result = mgr.get().unwrap();
        assert_eq!(expected, result);
    }
}
