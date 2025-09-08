//! ## Client Message
// (c) 2024-25 Ross Younger

use engineering_repr::EngineeringRepr as _;
use int_enum::IntEnum;
use num_traits::AsPrimitive;
use serde::{Deserialize, Serialize};
use serde_bare::Uint;

use super::{
    CongestionController, ConnectionType, PortRange_OnWire, ProtocolMessage, display_opt,
    display_opt_uint, display_vec_td,
};
use crate::{
    config::Configuration_Optional,
    protocol::{
        DataTag, FindTag as _, TaggedData, Variant, compat::Feature, control::Compatibility,
    },
    transport::ThroughputMode,
    util::serialization::SerializeAsString,
};

#[derive(
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Debug,
    Default,
    derive_more::Display,
    derive_more::From,
)]
/// The control parameters send from client to server.
pub enum ClientMessage {
    /// Special value indicating an endpoint has not yet read the remote `ClientMessage`.
    /// This value should never be seen on the wire.
    #[default]
    #[serde(skip_serializing)]
    ToFollow, // 0
    /// This version was introduced in qcp 0.3 with `VersionCompatibility` level 1.
    /// On the wire this is encoded with enum discriminant 1.
    V1(ClientMessageV1),
    /// This version was introduced with `VersionCompatibility` level 3.
    /// On the wire this is encoded with enum discriminant 2.
    V2(ClientMessageV2),
}
impl ProtocolMessage for ClientMessage {}

#[derive(
    Clone, Serialize, Deserialize, PartialEq, Default, derive_more::Debug, derive_more::Display,
)]
// We define a complicated Display here for efficiency; omit fields which are None.
#[display(
    "{connection_type}{}{}{}{}{}{}{}, attributes {}",
    display_opt("remote port", port.as_ref()),
    display_opt_uint("bw to client", bandwidth_to_client.as_ref()),
    display_opt_uint("bw to server", bandwidth_to_server.as_ref()),
    display_opt("RTT", rtt.as_ref()),
    display_opt("congestion algorithm ", congestion.as_ref()),
    display_opt_uint("cwnd ", initial_congestion_window.as_ref()),
    display_opt("timeout", timeout.as_ref()),
    display_vec_td(attributes),
)]
/// Version 1 of the client control parameters message.
/// This version was introduced in qcp 0.3 with `VersionCompatibility` level 1.
pub struct ClientMessageV1 {
    /// Client's self-signed certificate (DER)
    #[debug(ignore)]
    pub cert: Vec<u8>,
    /// The connection type to use (the type of socket we want the server to bind)
    pub connection_type: ConnectionType,
    /// If present, requests the server bind to a UDP port from a given range.
    pub port: Option<PortRange_OnWire>,
    /// Requests the server show its configuration for this connection
    pub show_config: bool,

    /// The requested bandwidth to use from client to server
    pub bandwidth_to_server: Option<Uint>,
    /// The requested bandwidth to use from server to client (if None, use the same as bandwidth to server)
    pub bandwidth_to_client: Option<Uint>,
    /// The network Round Trip Time, in milliseconds, to use in calculating the bandwidth delay product
    pub rtt: Option<u16>,
    /// The congestion control algorithm to use
    pub congestion: Option<CongestionController>,
    /// The initial congestion window, if specified
    pub initial_congestion_window: Option<Uint>,
    /// Connection timeout for the QUIC endpoints, in seconds
    pub timeout: Option<u16>,

    /// Optional extended attributes
    ///
    /// If it is mandatory for the server to action a given attribute, it MUST NOT be sent in this field.
    /// Instead, use a later version of the `ClientMessage`.
    ///
    /// This field was added in qcp 0.5 with `VersionCompatibility` level 2.
    /// Prior to Compatibility::Level(2) this was a reserved u8, which was required to be set to 0.
    /// If length 0, it looks the same on the wire.
    /// If length >0, earlier versions ignore the attributes.
    pub attributes: Vec<TaggedData<ClientMessageAttributes>>,
}

/// Extensible credentials tag
///
/// This enum was introduced with `VersionCompatibility` level 3.
#[derive(
    strum_macros::Display,
    Clone,
    Copy,
    Debug,
    Default,
    IntEnum,
    PartialEq,
    Serialize,
    Deserialize,
    clap::ValueEnum,
    strum_macros::EnumString,
    strum_macros::VariantNames,
)]
#[non_exhaustive]
#[repr(u64)]
#[strum(serialize_all = "lowercase")] // N.B. this applies to EnumString, not Display
#[serde(rename_all = "lowercase")]
pub enum CredentialsType {
    /// Indicates an invalid attribute, or that there is no preference.
    /// This enum variant is not valid on the wire; credentials must always be a concrete type.
    Any = 0,
    /// Self-signed X509 certificate.
    // Data is a `Variant::Bytes`
    X509,
    /// Raw (RFC7250) public key.
    // Data is a `Variant::Bytes`
    #[default]
    RawPublicKey,
}
impl DataTag for CredentialsType {}

#[derive(
    Clone, Default, Serialize, Deserialize, PartialEq, derive_more::Debug, derive_more::Display,
)]
#[display("{connection_type}, attributes {}", display_vec_td(attributes))]
/// Version 2 of the client control parameters message.
/// This version was introduced with `VersionCompatibility` level 3.
pub struct ClientMessageV2 {
    /// Client's TLS credentials (DER)
    #[debug(ignore)]
    pub credentials: TaggedData<CredentialsType>,

    /// The connection type to use (the type of socket we want the server to bind)
    pub connection_type: ConnectionType,

    /// Optional fields
    pub attributes: Vec<TaggedData<ClientMessage2Attributes>>,

    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}

impl ClientMessageV2 {
    fn new(credentials: TaggedData<CredentialsType>, connection_type: ConnectionType) -> Self {
        Self {
            credentials,
            connection_type,
            attributes: Vec::new(),
            extension: 0,
        }
    }
}

impl ClientMessageV2 {
    pub(crate) fn apply_config_attributes(
        &mut self,
        remote_config: bool,
        our_config: &Configuration_Optional,
    ) {
        if remote_config {
            self.attributes
                .push(ClientMessage2Attributes::OutputConfig.into());
        }
        if let Some(pr) = our_config.remote_port {
            self.attributes
                .push(ClientMessage2Attributes::PortRangeStart.with_unsigned(pr.begin));
            self.attributes
                .push(ClientMessage2Attributes::PortRangeEnd.with_unsigned(pr.end));
        }
        if let Some(eq) = our_config.tx {
            self.attributes
                .push(ClientMessage2Attributes::BandwidthToServer.with_unsigned(u64::from(eq)));
        }
        if let Some(eq) = our_config.rx {
            self.attributes
                .push(ClientMessage2Attributes::BandwidthToClient.with_unsigned(u64::from(eq)));
        }
        if let Some(rtt) = our_config.rtt {
            self.attributes
                .push(ClientMessage2Attributes::RoundTripTime.with_unsigned(rtt));
        }
        if let Some(cc) = our_config.congestion {
            self.attributes
                .push(ClientMessage2Attributes::CongestionControllerType.with_unsigned(*cc as u64));
        }
        if let Some(icw) = our_config.initial_congestion_window {
            self.attributes.push(
                ClientMessage2Attributes::InitialCongestionWindow.with_unsigned(u64::from(icw)),
            );
        }
        if let Some(t) = our_config.timeout {
            self.attributes
                .push(ClientMessage2Attributes::QuicTimeout.with_unsigned(t));
        }
        // DirectionOfTravel is set up by set_direction()
    }
}

impl From<ClientMessageV1> for ClientMessageV2 {
    fn from(v1: ClientMessageV1) -> Self {
        let mut attributes = Vec::new();
        if let Some(pr) = v1.port {
            attributes.push(ClientMessage2Attributes::PortRangeStart.with_unsigned(pr.begin));
            attributes.push(ClientMessage2Attributes::PortRangeEnd.with_unsigned(pr.end));
        }
        if v1.show_config {
            attributes.push(ClientMessage2Attributes::OutputConfig.into());
        }
        if let Some(Uint(bw)) = v1.bandwidth_to_server {
            attributes.push(ClientMessage2Attributes::BandwidthToServer.with_unsigned(bw));
        }
        if let Some(Uint(bw)) = v1.bandwidth_to_client {
            attributes.push(ClientMessage2Attributes::BandwidthToClient.with_unsigned(bw));
        }
        if let Some(rtt) = v1.rtt {
            attributes.push(ClientMessage2Attributes::RoundTripTime.with_unsigned(rtt));
        }
        if let Some(cc) = v1.congestion {
            attributes
                .push(ClientMessage2Attributes::CongestionControllerType.with_unsigned(cc as u64));
        }
        if let Some(Uint(icw)) = v1.initial_congestion_window {
            attributes.push(ClientMessage2Attributes::InitialCongestionWindow.with_unsigned(icw));
        }
        if let Some(t) = v1.timeout {
            attributes.push(ClientMessage2Attributes::QuicTimeout.with_unsigned(t));
        }
        if let Some(v) = v1
            .attributes
            .find_tag(ClientMessageAttributes::DirectionOfTravel)
        {
            attributes.push(ClientMessage2Attributes::DirectionOfTravel.with_variant(v.clone()));
        }
        Self {
            credentials: CredentialsType::X509.with_bytes(v1.cert),
            connection_type: v1.connection_type,
            attributes,
            extension: 0,
        }
    }
}

#[cfg(test)]
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug, Default)]
/// The control parameters send from client to server.
pub(super) enum OriginalClientMessage {
    #[default]
    #[serde(skip_serializing)]
    ToFollow,
    V1(OriginalClientMessageV1),
}
#[cfg(test)]
impl ProtocolMessage for OriginalClientMessage {}

#[cfg(test)]
#[derive(Clone, Serialize, Deserialize, PartialEq, Default, derive_more::Debug)]
pub(super) struct OriginalClientMessageV1 {
    pub(super) cert: Vec<u8>,
    pub(super) connection_type: ConnectionType,
    pub(super) port: Option<PortRange_OnWire>,
    pub(super) show_config: bool,
    pub(super) bandwidth_to_server: Option<Uint>,
    pub(super) bandwidth_to_client: Option<Uint>,
    pub(super) rtt: Option<u16>,
    pub(super) congestion: Option<CongestionController>,
    pub(super) initial_congestion_window: Option<Uint>,
    pub(super) timeout: Option<u16>,
    pub(super) extension: u8,
}

/// Extension attributes for [`ClientMessageV1`]
///
/// This enum was introduced in qcp 0.5 with `VersionCompatibility` level 2.
#[derive(strum_macros::Display, Clone, Copy, Debug, IntEnum, PartialEq)]
#[non_exhaustive]
#[repr(u64)]
pub enum ClientMessageAttributes {
    /// Indicates an invalid attribute.
    Invalid = 0,
    /// The intended direction of data flow for the connection.
    /// This is a value from [`Direction`], stored as [`crate::protocol::Variant::Unsigned`].
    DirectionOfTravel,
}
impl DataTag for ClientMessageAttributes {
    fn debug_data(&self, data: &Variant) -> String {
        match self {
            ClientMessageAttributes::DirectionOfTravel => {
                Direction::from_repr(data.coerce_unsigned().as_())
                    .unwrap_or(Direction::Both)
                    .to_string()
            }
            _ => format!("{data:?}"),
        }
    }
}

/// Extension attributes for `ClientMessageV2`
///
/// This enum was introduced with `VersionCompatibility` level 3.
#[derive(strum_macros::Display, Clone, Copy, Debug, IntEnum, PartialEq)]
#[non_exhaustive]
#[repr(u64)]
pub enum ClientMessage2Attributes {
    /// Indicates an invalid attribute.
    Invalid = 0,
    /// The intended direction of data flow for the connection.
    /// This is a value from [`Direction`], stored as [`crate::protocol::Variant::Unsigned`].
    DirectionOfTravel,
    /// Specifies the start of the port range we would like the server
    /// to bind to.
    /// Must always be used with `PortRangeEnd`.
    /// Data is [`crate::protocol::Variant::Unsigned`].
    PortRangeStart,
    /// Specifies the end of the port range we would like the server
    /// to bind to.
    /// Must always be used with `PortRangeStart`.
    /// Data is [`crate::protocol::Variant::Unsigned`].
    PortRangeEnd,
    /// Requests the server to output its config for this connection.
    /// Data is Empty.
    OutputConfig,
    /// The requested bandwidth to use from client to server.
    /// Data is [`crate::protocol::Variant::Unsigned`].
    BandwidthToServer,
    /// The requested bandwidth to use from server to client (if None, use the same as bandwidth to server).
    /// Data is [`crate::protocol::Variant::Unsigned`].
    BandwidthToClient,
    /// The network Round Trip Time, in milliseconds, to use in calculating the bandwidth delay product.
    /// Data is [`crate::protocol::Variant::Unsigned`].
    RoundTripTime,
    /// The congestion control algorithm to use.
    /// This is a value from [`CongestionController`], stored as [`crate::protocol::Variant::Unsigned`].
    CongestionControllerType,
    /// The initial congestion window.
    /// Data is [`crate::protocol::Variant::Unsigned`].
    InitialCongestionWindow,
    /// Connection timeout for the QUIC endpoints, in seconds.
    /// Data is [`crate::protocol::Variant::Unsigned`].
    QuicTimeout,
}
impl DataTag for ClientMessage2Attributes {
    fn debug_data(&self, data: &Variant) -> String {
        match self {
            ClientMessage2Attributes::DirectionOfTravel => {
                Direction::from_repr(data.coerce_unsigned().as_())
                    .unwrap_or(Direction::Both)
                    .to_string()
            }
            ClientMessage2Attributes::CongestionControllerType => {
                CongestionController::from_repr(data.coerce_unsigned().as_())
                    .unwrap_or_default()
                    .to_string()
            }
            ClientMessage2Attributes::BandwidthToClient
            | ClientMessage2Attributes::BandwidthToServer => {
                data.coerce_unsigned().to_eng(4).to_string()
            }
            _ => format!("{data:?}"),
        }
    }
}

/// Direction of data flow for the connection.
///
/// This enum was introduced in qcp 0.5 with `VersionCompatibility` level 2.
#[derive(
    strum_macros::Display, Clone, Copy, Debug, PartialEq, Eq, strum_macros::FromRepr, Default,
)]
#[allow(missing_docs)]
pub enum Direction {
    #[default]
    Both,
    ClientToServer,
    ServerToClient,
}
impl From<Direction> for Variant {
    fn from(value: Direction) -> Self {
        Variant::unsigned(value as u64)
    }
}
impl From<&Variant> for Direction {
    /// An infallible, type-coercing conversion.
    /// If the Variant is an unexpected type, returns the default (`Both`).
    fn from(value: &Variant) -> Self {
        Direction::from_repr(value.coerce_unsigned().as_()).unwrap_or_default()
    }
}
impl From<Option<&Variant>> for Direction {
    /// An infallible, type-coercing conversion.
    /// If the Variant is an unexpected type, returns the default (`Both`).
    fn from(value: Option<&Variant>) -> Self {
        value.map_or(Direction::default(), Direction::from)
    }
}
impl Direction {
    pub(crate) fn server_mode(self) -> ThroughputMode {
        match self {
            Direction::ClientToServer => ThroughputMode::Rx,
            Direction::ServerToClient => ThroughputMode::Tx,
            Direction::Both => ThroughputMode::Both,
        }
    }
    pub(crate) fn client_mode(self) -> ThroughputMode {
        match self {
            Direction::ClientToServer => ThroughputMode::Tx,
            Direction::ServerToClient => ThroughputMode::Rx,
            Direction::Both => ThroughputMode::Both,
        }
    }
}

impl ClientMessage {
    pub(crate) fn new(
        compat: Compatibility,
        cert: TaggedData<CredentialsType>,
        connection_type: ConnectionType,
        remote_config: bool,
        my_config: &Configuration_Optional,
    ) -> Self {
        assert!(cert.data.is_bytes());
        if compat.supports(Feature::CMSG_SMSG_2) {
            let mut msg = ClientMessageV2::new(cert, connection_type);
            msg.apply_config_attributes(remote_config, my_config);
            msg.into()
        } else {
            let cert_bytes = cert.data.into_bytes().unwrap_or_default();
            ClientMessageV1::new(&cert_bytes, connection_type, remote_config, my_config).into()
        }
    }

    pub(crate) fn set_direction(&mut self, direction: Direction) {
        match self {
            ClientMessage::ToFollow => (),
            ClientMessage::V1(msg) => msg
                .attributes
                .push(ClientMessageAttributes::DirectionOfTravel.with_unsigned(direction as u64)),
            ClientMessage::V2(msg) => msg
                .attributes
                .push(ClientMessage2Attributes::DirectionOfTravel.with_unsigned(direction as u64)),
        }
    }
}

impl ClientMessageV1 {
    pub(super) fn new(
        cert: &[u8],
        connection_type: ConnectionType,
        remote_config: bool,
        my_config: &Configuration_Optional,
    ) -> Self {
        use engineering_repr::EngineeringQuantity as EQ;
        // Configuration_Optional seems a bit much for recent rust-analyzer, but the compiler doesn't mind it. Sop:
        let rx: &Option<EQ<u64>> = &my_config.rx;
        let icw: &Option<EQ<u64>> = &my_config.initial_congestion_window;

        Self {
            cert: cert.to_vec(),
            connection_type,
            port: my_config.remote_port.map(std::convert::Into::into),
            show_config: remote_config,

            bandwidth_to_server: match my_config.tx.map(u64::from) {
                None | Some(0) => None,
                Some(v) => Some(Uint(v)),
            },
            bandwidth_to_client: rx.map(|u| Uint(u64::from(u))),
            rtt: my_config.rtt,
            congestion: my_config
                .congestion
                .map(|o: SerializeAsString<CongestionController>| *o),
            initial_congestion_window: icw.map(|u| Uint(u64::from(u))),
            timeout: my_config.timeout,

            attributes: vec![],
        }
    }
}

// /////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use assertables::{assert_contains, assert_matches};
    use pretty_assertions::{assert_eq, assert_str_eq};
    use serde_bare::Uint;

    use crate::{
        config::{Configuration_Optional, Manager},
        protocol::{
            DataTag, TaggedData,
            common::ProtocolMessage,
            control::{
                ClientMessage2Attributes, ClientMessageAttributes, ClientMessageV1,
                ClientMessageV2, Compatibility, CongestionController, ConnectionType,
                CredentialsType, Direction, OriginalClientMessage, OriginalClientMessageV1,
                test::dummy_cert,
            },
            display_vec_td,
        },
        util::PortRange as CliPortRange,
    };

    use super::{ClientMessage, PortRange_OnWire};

    #[test]
    fn serialize_client_message() {
        let config = Configuration_Optional {
            tx: Some(42u64.into()),
            rx: Some(89u64.into()),
            rtt: Some(1234),
            congestion: Some(CongestionController::Bbr.into()),
            udp_buffer: Some(456_789u64.into()),
            initial_congestion_window: Some(12345u64.into()),
            port: Some(CliPortRange { begin: 17, end: 98 }),
            remote_port: Some(CliPortRange {
                begin: 123,
                end: 456,
            }),
            remote_user: None,
            timeout: Some(432),
            // other client options are irrelevant to this test but we'll specify them anyway so we can rely on the compiler to catch any missing fields
            packet_threshold: None,
            time_threshold: None,
            initial_mtu: None,
            min_mtu: None,
            max_mtu: None,

            address_family: None,
            ssh: None,
            ssh_options: None,
            time_format: None,
            ssh_config: None,
            ssh_subsystem: None,
            color: None,
            tls_auth_type: None,
        };

        let cmsg = {
            let cert = CredentialsType::X509.with_bytes(dummy_cert());
            let mut manager = Manager::without_default(None);
            manager.merge_provider(&config);
            let cfg = manager.get::<Configuration_Optional>().unwrap();
            ClientMessage::new(
                Compatibility::Level(1),
                cert,
                ConnectionType::Ipv4,
                false,
                &cfg,
            )
        };
        let ser = cmsg.to_vec().unwrap();
        //println!("{cmsg:#?}");
        //println!("vec: {ser:?}");
        let deser = ClientMessage::from_slice(&ser).unwrap();
        //println!("{deser:#?}");

        let disp = format!("{cmsg}");
        eprintln!("{disp}");
        assert!(disp.contains("123-456"));

        let _empty: Vec<TaggedData<ClientMessageAttributes>> = vec![];
        assert_matches!(
            deser,
            ClientMessage::V1(ClientMessageV1 {
                cert: _,
                connection_type: ConnectionType::Ipv4,
                port: Some(PortRange_OnWire {
                    // crucial check: this is client config.remote_port
                    begin: 123,
                    end: 456
                }),
                show_config: false,
                bandwidth_to_server: Some(Uint(42)),
                bandwidth_to_client: Some(Uint(89)),
                rtt: Some(1234),
                congestion: Some(CongestionController::Bbr),
                initial_congestion_window: Some(Uint(12345)),
                timeout: Some(432),
                attributes: _empty,
            })
        );
    }

    #[test]
    fn construct_client_message() {
        // additional serialization cases not tested by serialize_and_provide_client_message
        let cert = CredentialsType::X509.with_bytes(dummy_cert());
        let mut manager = Manager::without_default(None);
        let config = Configuration_Optional::default();
        manager.merge_provider(&config);
        let cfg = manager.get::<Configuration_Optional>().unwrap();
        let cmsg = ClientMessage::new(
            Compatibility::Level(1),
            cert,
            ConnectionType::Ipv4,
            false,
            &cfg,
        );
        assert_matches!(
            cmsg,
            ClientMessage::V1(ClientMessageV1 {
                bandwidth_to_server: None,
                ..
            })
        );
    }

    #[test]
    fn wire_marshalling_client_message_v1() {
        let cert = dummy_cert();
        let msg = ClientMessage::V1(ClientMessageV1::new(
            &cert,
            ConnectionType::Ipv4,
            false,
            &Configuration_Optional::default(),
        ));
        let wire = msg.to_vec().unwrap();
        let expected = b"\x01\x03\x00\x01\x02\x04\x00\x00\x00\x00\x00\x00\x00\x00\x00".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn display_clientmessage_attrs() {
        let d = ClientMessageAttributes::DirectionOfTravel
            .with_variant(Direction::ClientToServer.into());
        let cm: ClientMessage = ClientMessage::V1(ClientMessageV1 {
            attributes: vec![d.clone()],
            ..Default::default()
        });
        // Debug
        let s = format!("{d:?}");
        eprintln!("{s}");
        assert_str_eq!(
            s,
            "TaggedData { tag: ClientMessageAttributes::DirectionOfTravel, data: ClientToServer, .. }"
        );
        let s = format!("{cm:?}");
        eprintln!("{s}");
        assert!(s.contains("ClientMessageAttributes::DirectionOfTravel, data: ClientToServer"));

        // Display
        let s = display_vec_td(&vec![d.clone()]);
        eprintln!("{s}");
        assert_str_eq!(s, "[DirectionOfTravel:ClientToServer]");
        let s = format!("{d}");
        eprintln!("{s}");
        assert_str_eq!(s, "(DirectionOfTravel, ClientToServer)");
        let s = format!("{cm}");
        eprintln!("{s}");
        assert!(s.contains("[DirectionOfTravel:ClientToServer]"));
    }

    #[test]
    fn clientmessagev1_attrs_backwards_compat() {
        let d = ClientMessageAttributes::DirectionOfTravel
            .with_variant(Direction::ClientToServer.into());
        let cm = ClientMessage::V1(ClientMessageV1 {
            attributes: vec![d.clone()],
            ..Default::default()
        });
        let wire = cm.to_vec().unwrap();
        let decode = OriginalClientMessage::from_slice(&wire).unwrap();
        // This is really a no-crash test.
        assert_eq!(
            decode,
            OriginalClientMessage::V1(OriginalClientMessageV1 {
                cert: vec![],
                connection_type: ConnectionType::Ipv4,
                port: None,
                show_config: false,
                bandwidth_to_server: None,
                bandwidth_to_client: None,
                rtt: None,
                congestion: None,
                initial_congestion_window: None,
                timeout: None,
                extension: 1, // Earlier versions ignore this field, so if the assert passes we're good.
            })
        );
    }

    #[test]
    fn client_message_2_debug_attrs() {
        let msg = ClientMessageV2 {
            attributes: vec![
                ClientMessage2Attributes::Invalid.into(),
                ClientMessage2Attributes::DirectionOfTravel
                    .with_unsigned(Direction::ClientToServer as u64),
                ClientMessage2Attributes::CongestionControllerType
                    .with_unsigned(CongestionController::NewReno as u64),
                ClientMessage2Attributes::OutputConfig.into(),
            ],
            ..Default::default()
        };
        let s = format!("{msg:?}");
        //eprintln!("{s}");
        assert!(s.contains("Invalid, data: Empty"));
        assert!(s.contains("DirectionOfTravel, data: ClientToServer"));
        assert!(s.contains("CongestionControllerType, data: newreno"));
        assert!(s.contains("OutputConfig, data: Empty"));
    }

    #[test]
    fn client_message_2_display() {
        let msg = ClientMessageV2 {
            attributes: vec![
                ClientMessage2Attributes::BandwidthToClient.with_unsigned(123_456_789u32),
                ClientMessage2Attributes::BandwidthToServer.with_unsigned(32_768u32),
                ClientMessage2Attributes::Invalid.into(),
                ClientMessage2Attributes::DirectionOfTravel
                    .with_unsigned(Direction::ClientToServer as u64),
                ClientMessage2Attributes::CongestionControllerType
                    .with_unsigned(CongestionController::NewReno as u64),
                ClientMessage2Attributes::OutputConfig.into(),
            ],
            ..Default::default()
        };
        let s = format!("{msg}");
        eprintln!("{s}");
        assert_contains!(s, "Invalid:Empty");
        assert_contains!(s, "BandwidthToClient:123.4M");
        assert_contains!(s, "BandwidthToServer:32.76k");
        assert_contains!(s, "DirectionOfTravel:ClientToServer");
        assert_contains!(s, "CongestionControllerType:newreno");
        assert_contains!(s, "OutputConfig:Empty");
    }
}
