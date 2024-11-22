//! Configuration file wrangling
// (c) 2024 Ross Younger

use super::Configuration;

use anyhow::Result;
use figment::{
    providers::{Format, Serialized, Toml},
    Figment, Provider,
};
use serde::Deserialize;
use std::path::PathBuf;

use tracing::{trace, warn};

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
    pub fn new() -> Self {
        let mut data = Figment::new().merge(Serialized::defaults(Configuration::default()));
        // TODO: systemwide config file ?
        data = add_user_config(data);
        Self {
            data,
            //..Self::default()
        }
    }

    /// Testing/internal constructor, does not read files from system
    #[must_use]
    #[allow(unused)]
    pub(crate) fn without_files() -> Self {
        let data = Figment::new().merge(Serialized::defaults(Configuration::default()));
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
        self.data = f.merge(provider);
    }

    /// Attempts to extract a particular struct from the data.
    ///
    /// Type `T` may be `Configuration` or one of its sub-elements.
    pub fn get<'de, T>(&self) -> anyhow::Result<T>
    where
        T: Deserialize<'de>,
    {
        Ok(self.data.extract()?)
    }
}

#[cfg(test)]
mod test {
    use crate::{
        config::manager::user_config_path,
        transport::{BandwidthParams, BandwidthParams_Optional},
    };

    use super::{Configuration, Manager};

    #[test]
    fn show_user_config_path() {
        println!("{:?}", user_config_path().unwrap());
    }

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
}
