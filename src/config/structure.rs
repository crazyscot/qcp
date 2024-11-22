//! Configuration structure
// (c) 2024 Ross Younger

use serde::{Deserialize, Serialize};

use crate::{transport::BandwidthParams, util::derive_deftly_template_Optionalify};
use derive_deftly::Deftly;

/// The top-level configuration/options structure for the QCP client side.
///
/// *This has a default() implementation returning the hard-wired config defaults!*
#[derive(Default, Deftly)]
#[derive_deftly(Optionalify)]
#[deftly(visibility = "pub(crate)")]
#[derive(Debug, Copy, Clone, PartialEq, Deserialize, Serialize)]
pub struct Configuration {
    /// Describes the bandwidth available to the system (or, at least, that we wish qcp to use)
    #[serde(flatten)]
    pub bandwidth: BandwidthParams,
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
