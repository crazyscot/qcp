//! Configures the QUIC transport layer from user settings
// (c) 2024 Ross Younger

use std::{sync::Arc, time::Duration};

use anyhow::Result;
use human_repr::HumanCount as _;
use num_traits::ToPrimitive as _;
use quinn::{
    MtuDiscoveryConfig, TransportConfig, VarInt,
    congestion::{BbrConfig, CubicConfig},
};
use tracing::{debug, trace};

use crate::{
    config::{self, Configuration, Configuration_Optional, Manager},
    protocol::control::{ClientMessageV1, CompatibilityLevel, CongestionController},
    util::PortRange,
};

/// Keepalive interval for the QUIC connection
pub(crate) const PROTOCOL_KEEPALIVE: Duration = Duration::from_secs(5);

const META_CLIENT: &str = "requested by client";
const META_NEGOTIATED: &str = "config resolution logic";

/// Specifies whether to configure to maximise transmission throughput, receive throughput, or both.
/// Specifying `Both` for a one-way data transfer will work, but wastes kernel memory.
#[derive(Copy, Clone, Debug, PartialEq, strum_macros::Display)]
pub enum ThroughputMode {
    /// We expect to send a lot but not receive
    Tx,
    /// We expect to receive a lot but not send much
    Rx,
    /// We expect to send and receive, or we don't know
    Both,
}

/// Creates a `quinn::TransportConfig` for the endpoint setup
pub fn create_config(
    params: &Configuration,
    mode: ThroughputMode,
    _compat: CompatibilityLevel,
) -> Result<Arc<TransportConfig>> {
    let mut mtu_cfg = MtuDiscoveryConfig::default();
    let _ = mtu_cfg.upper_bound(params.max_mtu);

    let mut config = TransportConfig::default();
    let _ = config
        .max_concurrent_bidi_streams(1u8.into())
        .max_concurrent_uni_streams(0u8.into())
        .keep_alive_interval(Some(PROTOCOL_KEEPALIVE))
        .allow_spin(true)
        .initial_rtt(2 * params.rtt_duration())
        .packet_threshold(params.packet_threshold)
        .time_threshold(params.time_threshold)
        .min_mtu(params.min_mtu)
        .initial_mtu(params.initial_mtu)
        .mtu_discovery_config(Some(mtu_cfg));

    let udp_buf = params
        .udp_buffer
        .to_usize()
        .ok_or(anyhow::anyhow!("udp_buffer size overflowed usize"))?;

    match mode {
        ThroughputMode::Tx | ThroughputMode::Both => {
            let _ = config
                .send_window(params.send_window())
                .datagram_send_buffer_size(udp_buf);
        }
        ThroughputMode::Rx => (),
    }

    match mode {
        // TODO: If we later support multiple streams at once, will need to consider receive_window and stream_receive_window.
        ThroughputMode::Rx | ThroughputMode::Both => {
            let rwnd: VarInt = params.recv_window().try_into()?;
            let _ = config
                .receive_window(rwnd) // Not strictly essential as quinn defaults to unlimited
                .stream_receive_window(rwnd) // essential; quinn defaults to 100Mbits x 100ms
                .datagram_receive_buffer_size(Some(udp_buf));
        }
        ThroughputMode::Tx => (),
    }

    let window = u64::from(params.initial_congestion_window);
    match params.congestion.0 {
        CongestionController::Cubic => {
            let mut cubic = CubicConfig::default();
            if window != 0 {
                let _ = cubic.initial_window(window);
            }
            let _ = config.congestion_controller_factory(Arc::new(cubic));
        }
        CongestionController::Bbr => {
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
    trace!("Quinn network configuration: {config:?}");

    let send_data = if mode == ThroughputMode::Rx {
        ""
    } else {
        &format!(
            "; send window {}, send buffer {}",
            params.send_window().human_count_bytes(),
            udp_buf.human_count_bytes()
        )
    };
    let recv_data = if mode == ThroughputMode::Tx {
        ""
    } else {
        &format!(
            "; recv window {}, recv buffer {}",
            params.recv_window().human_count_bytes(),
            udp_buf.human_count_bytes()
        )
    };
    debug!("Buffer configuration: mode {mode}{send_data}{recv_data}");

    Ok(config.into())
}

enum CombinationResponse<T> {
    Server,
    Client,
    Combined(T),
    Failure(anyhow::Error),
}

/// Negotiation logic for a single parameter. The two input types must be convertible.
///
/// If both server and client have a preference, the function `resolve_conflict` is invoked to determine the result.
///
/// # Output
/// This function has three possible outcomes:
/// * Add `key` and the client value to `client_out`, if the client configuration was selected
/// * Add `key` and the combined value to `resolved_out`, if there was a conflict and the result is a combined value
/// * Do nothing, if the server configuration was selected or if neither side expressed a preference.
///
fn negotiate_v3<ClientType, ServerType, BaseType>(
    client: Option<ClientType>,
    server: Option<ServerType>,
    resolve_conflict: fn(BaseType, BaseType) -> CombinationResponse<BaseType>,
    client_out: &mut config::Source,
    resolved_out: &mut config::Source,
    key: &str,
) -> Result<()>
where
    BaseType: From<ClientType> + From<ServerType>,
    ClientType: Clone + Into<figment::value::Value> + Into<BaseType> + Into<ServerType>,
    figment::value::Value: From<BaseType>,
    ServerType: std::cmp::PartialEq,
{
    match (client, server) {
        (None, None) => return Ok(()),
        (Some(cc), None) => {
            // only client specified; add to output
            client_out.add(key, cc.into());
        }
        (None, Some(_)) => (), // only server specified; it's already in our config layer
        (Some(cc), Some(ss)) => {
            if <ClientType as Into<ServerType>>::into(cc.clone()) == ss {
                // treat as server config
                return Ok(());
            }
            match resolve_conflict(cc.clone().into(), ss.into()) {
                CombinationResponse::Server => (),
                CombinationResponse::Client => {
                    client_out.add(key, cc.into());
                }
                CombinationResponse::Combined(val) => {
                    resolved_out.add(key, val.into());
                }
                CombinationResponse::Failure(err) => return Err(err),
            }
        }
    }
    Ok(())
}

fn min_ignoring_zero(cli: u64, srv: u64) -> CombinationResponse<u64> {
    match (cli, srv) {
        (0, _) => CombinationResponse::Server,
        (_, 0) => CombinationResponse::Client,
        (cc, ss) => CombinationResponse::Combined(std::cmp::min(cc, ss)),
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
/// | [Configuration] field  | [Control protocol](ClientMessageV1) field | Resolution |
/// | ---                    | ---                 | ---       |
/// | Client [`rx`](Configuration#structfield.rx) / Server [`tx`](Configuration#structfield.tx) | [`bandwidth_to_client`](ClientMessageV1#structfield.bandwidth_to_client) | Use the smaller of the two (ignoring zeroes) |
/// | Client [`tx`](Configuration#structfield.tx) / Server [`rx`](Configuration#structfield.rx) | [`bandwidth_to_server`](ClientMessageV1#structfield.bandwidth_to_server) | Use the smaller of the two (ignoring zeroes) |
/// | [`rtt`](Configuration#structfield.rtt) |  [`rtt`](ClientMessageV1#structfield.rtt) | Client preference wins |
/// | [`congestion`](Configuration#structfield.congestion) | [`congestion`](ClientMessageV1#structfield.congestion) | If the two prefs match, use that; if not, error |
/// | [`initial_congestion_window`](Configuration#structfield.initial_congestion_window) | [`initial_congestion_window`](ClientMessageV1#structfield.initial_congestion_window) | Client preference wins |
/// | [`timeout`](Configuration#structfield.timeout) | [`timeout`](ClientMessageV1#structfield.timeout) | Client preference wins |
/// | Client [`remote_port`](Configuration#structfield.remote_port) / Server [`port`](ClientMessageV1#structfield.port) | [`port`](ClientMessageV1#structfield.port) | Treat port `0` as "no preference". Compute the intersection of the two ranges. If they do not intersect, error. |
///
/// # Outputs
/// This function returns a fresh [`Configuration`] object holding the result of this logic.
///
/// In addition the input [`Manager`] is modified with _metadata_ to show the origin of each of the values.
///
/// # Errors
/// * If the input [`Manager`] is in the fused-error state
/// * If the resultant [`Configuration`] fails validation checks
/// * If the two configurations cannot be satisfactorily combined
///
pub fn combine_bandwidth_configurations(
    manager: &mut Manager,
    client: &ClientMessageV1,
) -> Result<Configuration> {
    let server: Configuration_Optional = manager.get::<Configuration_Optional>()?;
    let mut client_picks = config::Source::new(META_CLIENT);
    let mut negotiated = config::Source::new(META_NEGOTIATED);

    // a little syntactic sugar to reduce repetitions
    macro_rules! negotiate {
        ($cli:expr, $ser:expr, $resolve:expr, $key:expr) => {
            negotiate_v3(
                $cli,
                $ser,
                $resolve,
                &mut client_picks,
                &mut negotiated,
                $key,
            )
        };
    }

    // This is written from the server's point of view, i.e. bandwidth_to_server is server's rx.
    negotiate!(
        client.bandwidth_to_server.map(|u| u.0),
        server.rx,
        min_ignoring_zero,
        "rx"
    )?;
    negotiate!(
        client.bandwidth_to_client.map(|u| u.0),
        server.tx,
        min_ignoring_zero,
        "tx"
    )?;
    negotiate!(
        client.rtt,
        server.rtt,
        |_: u16, _| CombinationResponse::Client,
        "rtt"
    )?;
    negotiate!(
        client.congestion,
        server.congestion.map(|c| *c),
        |_: CongestionController, _| CombinationResponse::Failure(anyhow::anyhow!(
            "server and client have incompatible congestion algorithm requirements"
        )),
        "congestion"
    )?;
    negotiate!(
        client.initial_congestion_window.map(|u| u.0),
        server.initial_congestion_window,
        |_: u64, _| CombinationResponse::Server,
        "initial_congestion_window"
    )?;
    negotiate!(
        client.port.map(PortRange::from),
        server.port,
        |a, b| crate::util::PortRange::combine(a, b)
            .map_or_else(CombinationResponse::Failure, CombinationResponse::Combined),
        "port"
    )?;
    negotiate!(
        client.timeout,
        server.timeout,
        |_: u16, _| CombinationResponse::Client,
        "timeout"
    )?;

    // Convert selected fields to human-friendly representations
    make_dict_human_friendly(client_picks.borrow());
    make_dict_human_friendly(negotiated.borrow());

    manager.merge_provider(client_picks);
    manager.merge_provider(negotiated);
    manager.apply_system_default();

    manager.get::<Configuration>()?.validate()
}

fn make_entry_human_friendly(
    entry: std::collections::btree_map::Entry<'_, String, figment::value::Value>,
) {
    use engineering_repr::EngineeringRepr as _;
    use figment::value::Value;

    let _ = entry.and_modify(|v| {
        if let Value::Num(_tag, num) = v {
            if let Some(u) = num.to_u128() {
                *v = Value::from(u.to_eng(0).to_string());
            }
        }
    });
}

fn make_dict_human_friendly(dict: &mut figment::value::Dict) {
    make_entry_human_friendly(dict.entry("rx".into()));
    make_entry_human_friendly(dict.entry("tx".into()));
}
