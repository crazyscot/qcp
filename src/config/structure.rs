//! Configuration structure
// (c) 2024 Ross Younger

use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::{
    transport::CongestionControllerType,
    util::{derive_deftly_template_Optionalify, serde::HumanU64},
};
use derive_deftly::Deftly;

/// The top-level configuration/options structure for the QCP client side.
///
/// *This has a default() implementation returning the hard-wired config defaults!*
#[derive(Deftly)]
#[derive_deftly(Optionalify)]
#[deftly(visibility = "pub(crate)")]
#[derive(Debug, Copy, Clone, PartialEq, Parser, Deserialize, Serialize)]
pub struct Configuration {
    /// The maximum network bandwidth we expect receiving data FROM the remote system.
    /// This is expressed in bytes, with SI multipliers enabled (e.g. `10k`, `1G`, etc.)
    ///
    /// Note that this is a number of BYTES, not bits;
    /// if (for example) you expect to fill a 1Gbit ethernet connection,
    /// 125M might be a suitable setting.
    #[arg(
        long,
        help_heading("Network tuning"),
        display_order(10),
        value_name = "bytes"
    )]
    pub rx: HumanU64,

    /// The maximum network bandwidth we expect sending data TO the remote system,
    /// if it is different from the bandwidth FROM the system.
    /// (For example, when you are connected via an asymmetric last-mile DSL or fibre profile.)
    /// [default: use the value of --rx]
    #[arg(
        long,
        help_heading("Network tuning"),
        display_order(10),
        value_name = "bytes"
    )]
    pub tx: Option<HumanU64>,

    /// The expected network Round Trip time to the target system, in milliseconds.
    #[arg(
        short('r'),
        long,
        help_heading("Network tuning"),
        display_order(1),
        value_name("ms")
    )]
    pub rtt: u16,

    /// Specifies the congestion control algorithm to use.
    #[arg(
        long,
        action,
        value_name = "alg",
        help_heading("Advanced network tuning")
    )]
    #[clap(value_enum)]
    pub congestion: CongestionControllerType,

    /// (Network wizards only!)
    /// The initial value for the sending congestion control window.
    ///
    /// Setting this value too high reduces performance!
    ///
    /// If not specified, this setting is determined by the selected
    /// congestion control algorithm.
    #[arg(long, help_heading("Advanced network tuning"), value_name = "bytes")]
    pub initial_congestion_window: Option<u64>,
}

impl Default for Configuration {
    /// Hard-wired configuration defaults
    fn default() -> Self {
        Self {
            rx: 12_500_000.into(),
            tx: None,
            rtt: 300,
            congestion: CongestionControllerType::Cubic,
            initial_congestion_window: None,
        }
    }
}

#[cfg(test)]
mod test {
    use crate::config::Manager;
    use crate::transport::CongestionControllerType;
    use crate::util::serde::HumanU64;

    use super::Configuration;
    use figment::providers::{Format as _, Toml};
    use rand::distributions::{Distribution, Standard};
    use rand::Rng;

    impl Distribution<Configuration> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Configuration {
            Configuration {
                rx: rng.gen(),
                tx: Some(rng.gen()),
                rtt: rng.gen(),
                congestion: rng.gen(),
                initial_congestion_window: Some(rng.gen()),
            }
        }
    }

    impl Distribution<CongestionControllerType> for Standard {
        fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> CongestionControllerType {
            match rng.gen_range(0..=1) {
                0 => CongestionControllerType::Cubic,
                _ => CongestionControllerType::Bbr,
            }
        }
    }

    #[test]
    fn serde_pairwise_check() {
        let initial: Configuration = rand::random();
        assert_ne!(initial, Configuration::default());
        let ser = serde_json::to_string(&initial).unwrap();
        let deser: Configuration = serde_json::from_str(&ser).unwrap();
        //println!("{deser:#?}");
        assert_eq!(initial, deser);
    }

    const TEST_TOML: &str = r#"
    rx = "100k"
    tx = 123
    rtt = 42
    congestion = "Bbr"
    "#;

    const TEST_TOML_EXPECTED: Configuration = Configuration {
        rx: HumanU64(100_000),
        tx: Some(HumanU64(123)),
        rtt: 42,
        congestion: CongestionControllerType::Bbr,
        initial_congestion_window: None,
    };

    #[test]
    fn deser_from_toml() {
        let result: Configuration = toml::from_str(TEST_TOML).unwrap();
        assert_eq!(result, TEST_TOML_EXPECTED);
    }

    #[test]
    fn cfg_from_toml() {
        let mut mgr = Manager::without_files(None);
        mgr.merge(Toml::string(TEST_TOML));
        let result = mgr.get().unwrap();
        assert_eq!(result, TEST_TOML_EXPECTED);
    }
}
