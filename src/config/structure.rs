//! Configuration structure
// (c) 2024 Ross Younger

use serde::{Deserialize, Serialize};

use crate::{
    transport::{
        Configuration as TransportConfig, Configuration_Optional as TransportConfig_Optional,
    },
    util::{derive_deftly_template_FieldsList, derive_deftly_template_Optionalify, FieldsList},
};
use derive_deftly::Deftly;

/// The top-level configuration/options structure for the QCP client side.
///
/// **Note:** On this struct, `default()` returns qcp's hard-wired configuration defaults.
#[derive(Default, Deftly)]
#[derive_deftly(Optionalify)]
#[deftly(visibility = "pub(crate)")]
#[derive_deftly(FieldsList)]
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Configuration {
    /// Parameters affecting the transport. System bandwidth, UDP ports, timeout.
    #[serde(flatten)]
    pub bandwidth: TransportConfig,
    /// Configurable options specific to client side
    #[serde(flatten)]
    pub client: crate::client::ClientConfiguration,
}

#[cfg(test)]
mod test {
    use super::Configuration;

    #[test]
    fn flattened() {
        let v = Configuration::default();
        let j = serde_json::to_string(&v).unwrap();
        let d = json::parse(&j).unwrap();
        assert!(!d.has_key("bw"));
        assert!(d.has_key("rtt"));
    }
}
