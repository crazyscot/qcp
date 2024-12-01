//! Configuration structure
// (c) 2024 Ross Younger

use serde::{Deserialize, Serialize};

use crate::{
    transport::{BandwidthParams, BandwidthParams_Optional, QuicParams, QuicParams_Optional},
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
    /// Describes the bandwidth available to the system (or, at least, that we wish qcp to use)
    #[serde(flatten)]
    pub bandwidth: BandwidthParams,
    /// Parameters affecting the QUIC endpoint
    #[serde(flatten)]
    pub quic: QuicParams,
    /// Configurable options specific to client side
    #[serde(flatten)]
    pub client: crate::client::Options,
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
