//! System default configuration provider
// (c) 2024 Ross Younger

use crate::Configuration;
use figment::{Metadata, Provider, providers::Serialized};

/// A [`figment::Provider`](https://docs.rs/figment/latest/figment/trait.Provider.html) that holds
/// the set of system default options
pub(super) struct SystemDefault {}

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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::{Configuration, config::Manager};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_system_default() {
        let mut mgr = Manager::new(None, false, false);
        let defs = Configuration::system_default();

        mgr.apply_system_default();
        let cfg = mgr.get::<Configuration>().unwrap();
        assert_eq!(cfg.tx, defs.tx);
        assert_eq!(cfg.rx, defs.rx);
        assert_eq!(cfg.rtt, defs.rtt);
        assert_eq!(cfg.port, defs.port);
    }
}
