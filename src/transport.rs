//! QUIC transport configuration
// (c) 2024 Ross Younger

use std::{convert::Infallible, str::FromStr, sync::Arc, time::Duration};

use anyhow::Result;
use human_repr::HumanCount as _;
use quinn::{
    congestion::{BbrConfig, CubicConfig},
    TransportConfig,
};
use serde::{de, Deserialize, Serialize};
use strum::VariantNames;
use tracing::debug;

use crate::{
    config::{Configuration, Configuration_Optional, Manager},
    protocol::control::ClientMessageV1,
};

/// Keepalive interval for the QUIC connection
pub const PROTOCOL_KEEPALIVE: Duration = Duration::from_secs(5);

/// Specifies whether to configure to maximise transmission throughput, receive throughput, or both.
/// Specifying `Both` for a one-way data transfer will work, but wastes kernel memory.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ThroughputMode {
    /// We expect to send a lot but not receive
    Tx,
    /// We expect to receive a lot but not send much
    Rx,
    /// We expect to send and receive, or we don't know
    Both,
}

/// Selects the congestion control algorithm to use.
/// On the wire, this is serialized as a standard BARE enum.
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    strum::Display,
    strum::EnumString,
    strum::FromRepr,
    strum::VariantNames,
    clap::ValueEnum,
    Serialize,
)]
#[strum(serialize_all = "lowercase")] // N.B. this applies to EnumString, not Display
#[repr(u8)]
pub enum CongestionControllerType {
    /// The congestion algorithm TCP uses. This is good for most cases.
    #[default]
    Cubic = 0,
    /// (Use with caution!) An experimental algorithm created by Google,
    /// which increases goodput in some situations
    /// (particularly long and fat connections where the intervening
    /// buffers are shallow). However this comes at the cost of having
    /// more data in-flight, and much greater packet retransmission.
    /// See
    /// `https://blog.apnic.net/2020/01/10/when-to-use-and-not-use-bbr/`
    /// for more discussion.
    Bbr = 1,
}

impl<'de> Deserialize<'de> for CongestionControllerType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let lower = s.to_ascii_lowercase();
        // requires strum::EnumString && strum::VariantNames && #[strum(serialize_all = "lowercase")]
        FromStr::from_str(&lower)
            .map_err(|_| de::Error::unknown_variant(&s, CongestionControllerType::VARIANTS))
    }
}

/// Creates a `quinn::TransportConfig` for the endpoint setup
pub fn create_config(params: &Configuration, mode: ThroughputMode) -> Result<Arc<TransportConfig>> {
    let mut config = TransportConfig::default();
    let _ = config
        .max_concurrent_bidi_streams(1u8.into())
        .max_concurrent_uni_streams(0u8.into())
        .keep_alive_interval(Some(PROTOCOL_KEEPALIVE))
        .allow_spin(true);

    match mode {
        ThroughputMode::Tx | ThroughputMode::Both => {
            let _ = config
                .send_window(params.send_window())
                .datagram_send_buffer_size(Configuration::send_buffer().try_into()?);
        }
        ThroughputMode::Rx => (),
    }
    #[allow(clippy::cast_possible_truncation)]
    match mode {
        // TODO: If we later support multiple streams at once, will need to consider receive_window and stream_receive_window.
        ThroughputMode::Rx | ThroughputMode::Both => {
            let _ = config
                .stream_receive_window(params.recv_window().try_into()?)
                .datagram_receive_buffer_size(Some(Configuration::recv_buffer() as usize));
        }
        ThroughputMode::Tx => (),
    }

    let window = params.initial_congestion_window;
    match params.congestion {
        CongestionControllerType::Cubic => {
            let mut cubic = CubicConfig::default();
            if window != 0 {
                let _ = cubic.initial_window(window);
            }
            let _ = config.congestion_controller_factory(Arc::new(cubic));
        }
        CongestionControllerType::Bbr => {
            let mut bbr = BbrConfig::default();
            if window != 0 {
                let _ = bbr.initial_window(window);
            }
            let _ = config.congestion_controller_factory(Arc::new(bbr));
        }
    }

    debug!(
        "Final network configuration: {}",
        params.format_transport_config()
    );
    debug!(
        "Buffer configuration: send window {sw}, buffer {sb}; recv window {rw}, buffer {rb}",
        sw = params.send_window().human_count_bytes(),
        sb = Configuration::send_buffer().human_count_bytes(),
        rw = params.recv_window().human_count_bytes(),
        rb = Configuration::recv_buffer().human_count_bytes()
    );

    Ok(config.into())
}

/// Negotiation logic for a single parameter where they are both the same type
fn negotiate<T, E>(
    a: Option<T>,
    b: Option<T>,
    default: T,
    conflict: fn(T, T) -> Result<T, E>,
) -> Result<T, E> {
    match (a, b) {
        (None, None) => Ok(default),
        (Some(aa), None) => Ok(aa),
        (None, Some(bb)) => Ok(bb),
        (Some(aa), Some(bb)) => conflict(aa, bb),
    }
}

/// Negotiation logic for a single parameter where they are different but convertible types.
/// The type of `default` governs the output type.
fn negotiate_mixed<T, U, D, E>(
    a: Option<T>,
    b: Option<U>,
    default: D,
    conflict: fn(D, D) -> Result<D, E>,
) -> Result<D, E>
where
    D: From<T> + From<U>,
{
    match (a, b) {
        (None, None) => Ok(default),
        (Some(aa), None) => Ok(aa.into()),
        (None, Some(bb)) => Ok(bb.into()),
        (Some(aa), Some(bb)) => conflict(aa.into(), bb.into()),
    }
}

/// Applies the bandwidth/parameter negotiation logic given the server's configuration (`server`) and the client's requests (`client`).
///
/// # Logic
/// The general rules are:
/// * All parameters are optional from both sides.
/// * If one side does not express a preference for a parameter, the other side's preference automatically wins.
/// * If neither side specifies a given parameter, the system default shall obtain.
/// * If both sides specify a preference, consult the following table to determine how to resolve the situation.
///
/// ## Parameter resolution
///
///
/// | [Configuration] field  | [Control protocol](ClientMessageV1) | Resolution |
/// | ---                    | ---                 | ---       |
/// | Client [`rx`](Configuration#structfield.rx) / Server [`tx`](Configuration#structfield.tx) | [`bandwidth_to_client`](ClientMessageV1#structfield.bandwidth_to_client) | Use the smaller of the two |
/// | Client [`tx`](Configuration#structfield.tx) / Server [`rx`](Configuration#structfield.rx) | [`bandwidth_to_server`](ClientMessageV1#structfield.bandwidth_to_server) | Use the smaller of the two |
/// | [`rtt`](Configuration#structfield.rtt) |  [`rtt`](ClientMessageV1#structfield.rtt) | Client preference wins |
/// | [`congestion`](Configuration#structfield.congestion) | [`congestion`](ClientMessageV1#structfield.congestion) | If the two prefs match, use that; if not, error |
/// | [`initial_congestion_window`](Configuration#structfield.initial_congestion_window) | [`initial_congestion_window`](ClientMessageV1#structfield.initial_congestion_window) | Client preference wins |
/// | [`timeout`](Configuration#structfield.timeout) | [`timeout`](ClientMessageV1#structfield.timeout) | Client preference wins |
/// | Client [`remote_port`](Configuration#structfield.remote_port) / Server [`port`](ClientMessageV1#structfield.port) | [`port`](ClientMessageV1#structfield.port) | Treat port `0` as "no preference". Compute the intersection of the two ranges. If they do not intersect, error. |
///
/// # Return value
/// A fresh [`Configuration`] object holding the result of this logic.
///
/// # Errors
/// * If the input [`Manager`] is in the fused-error state
/// * If the resultant [`Configuration`] fails validation checks
///
pub fn combine_bandwidth_configurations(
    manager: &Manager,
    client: &ClientMessageV1,
) -> Result<Configuration> {
    #[allow(clippy::unnecessary_wraps)]
    fn client_wins<T>(_server: T, client: T) -> Result<T, Infallible> {
        Ok(client)
    }

    let server: Configuration_Optional = manager.get::<Configuration_Optional>()?;
    let defaults = Configuration::system_default();

    let result = Configuration {
        rx: negotiate_mixed(
            server.rx,
            client.bandwidth_to_server.map(|u| u.0),
            u64::from(defaults.rx),
            |a, b| Ok::<u64, Infallible>(std::cmp::min(a, b)),
        )?
        .into(),
        tx: negotiate_mixed(
            server.tx,
            client.bandwidth_to_client.map(|u| u.0),
            u64::from(defaults.tx),
            |a, b| Ok::<u64, Infallible>(std::cmp::min(a, b)),
        )?
        .into(),
        rtt: negotiate(server.rtt, client.rtt, defaults.rtt, client_wins)?,
        congestion: negotiate_mixed(
            server.congestion,
            client.congestion,
            defaults.congestion,
            |_, _| {
                anyhow::bail!(
                    "server and client have incompatible congestion algorithm requirements"
                )
            },
        )?,
        initial_congestion_window: negotiate_mixed(
            server.initial_congestion_window,
            client.initial_congestion_window.map(|u| u.0),
            defaults.initial_congestion_window,
            |a, _| Ok::<u64, Infallible>(a),
        )?,
        port: negotiate_mixed(
            server.port,
            client.port,
            defaults.port,
            crate::util::PortRange::combine,
        )?,
        timeout: negotiate(
            server.timeout,
            client.timeout,
            defaults.timeout,
            client_wins,
        )?,

        // Other fields are irrelevant to the negotiation.
        // We do not use ..defaults here, as we want the compiler to catch if/when a new field is added.
        address_family: defaults.address_family,
        ssh: defaults.ssh.clone(),
        ssh_options: defaults.ssh_options.clone(),
        remote_port: defaults.remote_port,
        time_format: defaults.time_format,
        ssh_config: defaults.ssh_config.clone(),
    };

    result.validate()
}
