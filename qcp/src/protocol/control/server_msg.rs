//! ## Server Message
// (c) 2024-25 Ross Younger

use engineering_repr::EngineeringRepr as _;
use figment::{
    Profile, Provider,
    value::{Dict, Map},
};
use int_enum::IntEnum;
use num_traits::AsPrimitive;
use serde::{Deserialize, Serialize};
use serde_bare::Uint;

use super::ProtocolMessage;
use crate::{
    Configuration,
    protocol::{
        DataTag, TaggedData,
        compat::Feature,
        control::{Compatibility, CongestionController, CredentialsType},
        display_vec_td,
    },
};

#[derive(
    Clone,
    Serialize,
    Default,
    Deserialize,
    PartialEq,
    Debug,
    derive_more::From,
    derive_more::Display,
)]
/// The control parameters sent from server to client
pub enum ServerMessage {
    /// Special value indicating an endpoint has not yet read the remote `ServerMessage`.
    /// This value should never be seen on the wire.
    #[default]
    #[serde(skip_serializing)]
    ToFollow,
    /// This version was introduced in qcp 0.3 with `VersionCompatibility` level 1.
    /// On the wire enum discriminant: 1.
    V1(ServerMessageV1),
    /// This message type was introduced in qcp 0.3 with `VersionCompatibility` level 1.
    /// On the wire enum discriminant: 2.
    Failure(ServerFailure),
    /// This message type was introduced with `VersionCompatibility` level 3.
    /// On the wire enum discriminant: 3.
    V2(ServerMessageV2),
}
impl ProtocolMessage for ServerMessage {}

impl ServerMessage {
    pub(crate) fn new(
        compat: Compatibility,
        config: &Configuration,
        port: u16,
        credentials: TaggedData<CredentialsType>,
        common_name: String,
        warning: String,
    ) -> Self {
        assert!(credentials.data.is_bytes());
        let bandwidth_to_server = Uint(config.rx());
        let bandwidth_to_client = Uint(config.tx());
        if compat.supports(Feature::CMSG_SMSG_2) {
            let mut msg = ServerMessageV2 {
                port,
                credentials,
                common_name,
                bandwidth_to_server,
                bandwidth_to_client,
                rtt: config.rtt,
                ..Default::default()
            };
            msg.apply_config_attributes(config);
            msg.into()
        } else {
            let cert_bytes = credentials.data.into_bytes().unwrap_or_default();
            ServerMessageV1 {
                port,
                cert: cert_bytes,
                name: common_name,
                bandwidth_to_server,
                bandwidth_to_client,
                rtt: config.rtt,
                congestion: config.congestion,
                initial_congestion_window: Uint(config.initial_congestion_window),
                timeout: config.timeout,
                warning,
                ..Default::default()
            }
            .into()
        }
    }
}

#[derive(
    Clone, Serialize, Deserialize, PartialEq, Eq, derive_more::Debug, Default, derive_more::Display,
)]
/// Version 1 of the message from server to client.
/// This version was introduced in qcp 0.3 with `VersionCompatibility` level 1.
#[display(
    "{name}:{port} in {}, out {}, rtt {rtt}, congestion {congestion}/{}, timeout {timeout}, \"{warning}\"",
    bandwidth_to_server.0,
    bandwidth_to_client.0,
    initial_congestion_window.0,
)]
pub struct ServerMessageV1 {
    /// UDP port the server has bound to
    pub port: u16,
    /// Server's self-signed certificate (DER)
    #[debug(ignore)]
    pub cert: Vec<u8>,
    /// Name in the server cert (this saves us having to unpick it from the certificate)
    pub name: String,

    /// The final bandwidth to use from client to server
    pub bandwidth_to_server: Uint,
    /// The final bandwidth to use from server to client
    pub bandwidth_to_client: Uint,
    /// The final round-trip-time to use on the connection
    pub rtt: u16,
    /// The congestion control algorithm to use
    pub congestion: CongestionController,
    /// The initial congestion window to use (0 means "use algorithm default")
    pub initial_congestion_window: Uint,
    /// Connection timeout for the QUIC endpoints, in seconds
    pub timeout: u16,

    /// If non-zero length, this is a warning message to be relayed to a human
    pub warning: String,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}

impl ServerMessageV1 {
    const META_NAME: &str = "server message";
}

impl Provider for ServerMessageV1 {
    fn metadata(&self) -> figment::Metadata {
        figment::Metadata::named(Self::META_NAME)
    }

    fn data(&self) -> Result<figment::value::Map<figment::Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();
        let mut insert = |key: &str, val: figment::value::Value| {
            let _ = dict.insert(key.into(), val);
        };
        // This is written from the consumer's (client's) point of view, i.e. bandwidth_to_server is client's tx.
        insert("tx", self.bandwidth_to_server.0.into());
        insert("rx", self.bandwidth_to_client.0.into());

        insert("rtt", self.rtt.into());
        insert("congestion", self.congestion.to_string().into());
        insert("timeout", self.timeout.into());
        insert(
            "initial_congestion_window",
            self.initial_congestion_window.0.into(),
        );

        let mut profile_map = Map::new();
        let _ = profile_map.insert(Profile::Global, dict);

        Ok(profile_map)
    }
}

/// A special type of message indicating that an error occurred and the connection cannot proceed.
///
/// Protocol Version Compatibility: V1
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, derive_more::Display)]
pub enum ServerFailure {
    /// The server failed to understand control channel traffic received from the client.
    ///
    /// Protocol Version Compatibility: V1
    Malformed,
    /// The client's configuration and server's configuration could not be reconciled.
    /// The string within explains why.
    ///
    /// Protocol Version Compatibility: V1
    #[display("Negotiation Failed: {_0}")]
    NegotiationFailed(String),
    /// The QUIC endpoint could not be set up.
    /// The string within contains more detail.
    ///
    /// Protocol Version Compatibility: V1
    #[display("Endpoint Failed: {_0}")]
    EndpointFailed(String),
    /// An unknown error occurred. This is a catch-all for forward compatibility.
    ///
    /// Protocol Version Compatibility: V1
    #[display("Unknown error: {_0}")]
    Unknown(String),
}

/// Version 2 of the server control parameters message.
/// This version was introduced with `VersionCompatibility` level 3.
#[derive(
    Clone, Serialize, Default, Deserialize, PartialEq, derive_more::Debug, derive_more::Display,
)]
#[display(
    "{common_name}:{port} in {}, out {}, rtt {rtt}, attrs {}",
    bandwidth_to_server.0.to_eng(4),
    bandwidth_to_client.0.to_eng(4),
    display_vec_td(attributes)
)]
pub struct ServerMessageV2 {
    /// UDP port the server has bound to
    pub port: u16,
    /// Server's TLS credentials (DER encoded)
    #[debug(ignore)]
    pub credentials: TaggedData<CredentialsType>,

    /// Server's common name, if provided in the credentials.
    /// This saves us having to unpick it from the certificate.
    pub common_name: String,

    /// The final bandwidth to use from client to server
    pub bandwidth_to_server: Uint,
    /// The final bandwidth to use from server to client
    pub bandwidth_to_client: Uint,
    /// The final round-trip-time to use on the connection
    pub rtt: u16,

    /// Optional fields
    pub attributes: Vec<TaggedData<ServerMessage2Attributes>>,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}

impl From<ServerMessageV1> for ServerMessageV2 {
    fn from(v1: ServerMessageV1) -> Self {
        let mut attributes = Vec::new();
        if !v1.warning.is_empty() {
            attributes.push(ServerMessage2Attributes::WarningMessage.with_str(v1.warning));
        }
        attributes.push(
            ServerMessage2Attributes::CongestionController.with_unsigned(v1.congestion as u64),
        );
        if v1.initial_congestion_window.0 != 0 {
            attributes.push(
                ServerMessage2Attributes::InitialCongestionWindow
                    .with_unsigned(v1.initial_congestion_window.0),
            );
        }
        if v1.timeout != 0 {
            attributes.push(ServerMessage2Attributes::QuicTimeout.with_unsigned(v1.timeout));
        }
        Self {
            port: v1.port,
            credentials: CredentialsType::X509.with_bytes(v1.cert),
            common_name: v1.name,
            bandwidth_to_server: v1.bandwidth_to_server,
            bandwidth_to_client: v1.bandwidth_to_client,
            rtt: v1.rtt,
            attributes,
            extension: 0,
        }
    }
}

/// Optional attributes for `ServerMessageV2`
///
/// This enum was introduced with `VersionCompatibility` level 3.
#[derive(strum_macros::Display, Clone, Copy, Debug, IntEnum, PartialEq)]
#[non_exhaustive]
#[repr(u64)]
pub enum ServerMessage2Attributes {
    /// Indicates an invalid attribute.
    Invalid = 0,

    /// The congestion control algorithm to use.
    /// This is a value from [`CongestionController`], stored as [`crate::protocol::Variant::Unsigned`].
    ///
    /// The [`ServerMessage2Attributes::InitialCongestionWindow`] attribute may be used to specify
    /// the initial congestion window, if required.
    CongestionController,

    /// The initial congestion window to use. If not present, the algorithm default is used.
    /// Data is [`crate::protocol::Variant::Unsigned`].
    InitialCongestionWindow,

    /// A warning message to be relayed to a human.
    /// Data is [`crate::protocol::Variant::String`].
    WarningMessage,

    /// Connection timeout for the QUIC endpoints, in seconds.
    /// Data is [`crate::protocol::Variant::Unsigned`].
    QuicTimeout,
}

impl DataTag for ServerMessage2Attributes {}

impl ServerMessageV2 {
    pub(crate) fn apply_config_attributes(&mut self, config: &Configuration) {
        if config.congestion != CongestionController::default() {
            self.attributes.push(
                ServerMessage2Attributes::CongestionController
                    .with_unsigned(config.congestion as u64),
            );
        }
        let window = config.initial_congestion_window;
        if window != 0 {
            self.attributes
                .push(ServerMessage2Attributes::InitialCongestionWindow.with_unsigned(window));
        }
        if config.timeout != 0 {
            self.attributes
                .push(ServerMessage2Attributes::QuicTimeout.with_unsigned(config.timeout));
        }
        // WarningMessage is set up when the message is created.
    }
}

impl Provider for ServerMessageV2 {
    fn metadata(&self) -> figment::Metadata {
        figment::Metadata::named("ServerMessageV2")
    }

    fn data(&self) -> Result<figment::value::Map<figment::Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();
        let mut insert = |key: &str, val: figment::value::Value| {
            let _ = dict.insert(key.into(), val);
        };
        // This is written from the consumer's (client's) point of view, i.e. bandwidth_to_server is client's tx.
        insert("tx", self.bandwidth_to_server.0.into());
        insert("rx", self.bandwidth_to_client.0.into());
        insert("rtt", self.rtt.into());

        for attr in &self.attributes {
            if let Some(tag) = attr.tag() {
                let data = &attr.data;
                match tag {
                    ServerMessage2Attributes::CongestionController => {
                        let ctrl = data.coerce_unsigned().as_();
                        let cc = CongestionController::from_repr(ctrl).unwrap_or_default();
                        insert("congestion", cc.into());
                    }
                    ServerMessage2Attributes::InitialCongestionWindow => {
                        insert("initial_congestion_window", data.coerce_unsigned().into());
                    }
                    ServerMessage2Attributes::QuicTimeout => {
                        insert("timeout", data.coerce_unsigned().into());
                    }
                    // attributes not forming part of the configuration:
                    ServerMessage2Attributes::WarningMessage
                    | ServerMessage2Attributes::Invalid => {}
                }
            } else {
                return Err(figment::Error::from(format!(
                    "Unknown ServerMessage2Attributes tag {}",
                    attr.tag_raw()
                )));
            }
        }

        let mut profile_map = Map::new();
        let _ = profile_map.insert(Profile::Global, dict);

        Ok(profile_map)
    }
}

// /////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use assertables::assert_contains;
    use pretty_assertions::assert_eq;
    use serde_bare::Uint;

    use crate::{
        config::{Configuration, Configuration_Optional, Manager},
        protocol::{
            DataTag, FindTag,
            common::ProtocolMessage,
            control::{
                CongestionController, ServerMessage2Attributes, ServerMessageV1, ServerMessageV2,
                test::dummy_credentials,
            },
        },
    };

    use super::{ServerFailure, ServerMessage};

    #[test]
    fn serialize_provide_server_message() {
        let v1 = ServerMessageV1 {
            port: 12345,
            cert: vec![9, 8, 7],
            name: "hello".to_string(),
            bandwidth_to_client: Uint(123),
            bandwidth_to_server: Uint(456),
            rtt: 789,
            congestion: CongestionController::Bbr,
            initial_congestion_window: Uint(4321),
            timeout: 42,
            warning: String::from("this is a warning"),
            extension: 0,
        };
        let msg = ServerMessage::V1(v1.clone());
        let wire = msg.to_vec().unwrap();
        let deser = ServerMessage::from_slice(&wire).unwrap();
        assert_eq!(msg, deser);

        let mut manager = Manager::without_files(None); // with system defaults
        manager.merge_provider(&v1);
        let cfg = manager.get::<Configuration>().unwrap();
        println!("{cfg:?}");
        let expected = Configuration {
            // Server message is processed by the client, so bandwidth_to_client becomes config.rx
            rx: v1.bandwidth_to_client.0,
            tx: v1.bandwidth_to_server.0,
            rtt: v1.rtt,
            congestion: v1.congestion,
            initial_congestion_window: v1.initial_congestion_window.0,
            timeout: v1.timeout,
            ..Configuration::system_default().clone()
        };
        assert_eq!(cfg, expected);
    }
    #[test]
    fn skip_serializing() {
        let msg = ServerMessage::ToFollow;
        let _ = msg.to_vec().expect_err("ToFollow cannot be serialized");
    }

    #[test]
    fn display_server_failure() {
        let sf = ServerFailure::Malformed;
        assert_eq!(format!("{sf}"), "Malformed");
        let sf = ServerFailure::NegotiationFailed("hello".to_string());
        assert_eq!(format!("{sf}"), "Negotiation Failed: hello");
        let sf = ServerFailure::EndpointFailed("hello".to_string());
        assert_eq!(format!("{sf}"), "Endpoint Failed: hello");
        let sf = ServerFailure::Unknown("hello".to_string());
        assert_eq!(format!("{sf}"), "Unknown error: hello");
    }

    #[test]
    fn wire_marshalling_server_message_v1() {
        let msg = ServerMessage::V1(ServerMessageV1 {
            port: 12345,
            cert: vec![9, 8, 7],
            name: "hello".to_string(),
            bandwidth_to_client: Uint(123),
            bandwidth_to_server: Uint(456),
            rtt: 789,
            congestion: CongestionController::Bbr,
            initial_congestion_window: Uint(4321),
            timeout: 42,
            warning: String::from("this is a warning"),
            extension: 0,
        });
        let wire = msg.to_vec().unwrap();
        let expected = b"\x0190\x03\x09\x08\x07\x05hello\xc8\x03{\x15\x03\x01\xe1!*\x00\x11this is a warning\x00".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_server_message_failure() {
        let msg = ServerMessage::Failure(ServerFailure::NegotiationFailed("hello".to_string()));
        let wire = msg.to_vec().unwrap();
        let expected = b"\x02\x01\x05hello".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_server_message_v2() {
        let credentials = dummy_credentials();
        let msg2 = ServerMessageV2 {
            port: 1234,
            credentials,
            common_name: "srv".into(),
            bandwidth_to_server: Uint(12),
            bandwidth_to_client: Uint(125_000_000),
            rtt: 50,
            attributes: vec![
                ServerMessage2Attributes::WarningMessage.with_str("hi"),
                ServerMessage2Attributes::QuicTimeout.with_unsigned(4u8),
            ],
            extension: 0,
        };
        let msg = ServerMessage::V2(msg2);
        let wire = msg.to_vec().unwrap();
        let expected =
            b"\x03\xd2\x04\x01\x05\x03\x00\x01\x02\x03srv\x0c\xc0\xb2\xcd;2\x00\x02\x03\x04\x02hi\x04\x03\x04\x00".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn server_message_1_2_conversion() {
        use FindTag as _;

        let mut msg1 = ServerMessageV1::default();
        msg1.warning.push('a');
        msg1.initial_congestion_window.0 = 42;
        msg1.timeout = 7;

        let msg2 = ServerMessageV2::from(msg1);
        let attrs = &msg2.attributes;

        let tag = attrs
            .find_tag(ServerMessage2Attributes::WarningMessage)
            .unwrap();
        assert_eq!(tag.as_str(), Some("a"));

        let tag = attrs
            .find_tag(ServerMessage2Attributes::InitialCongestionWindow)
            .unwrap();
        assert_eq!(tag.coerce_unsigned(), 42);

        let tag = attrs
            .find_tag(ServerMessage2Attributes::QuicTimeout)
            .unwrap();
        assert_eq!(tag.coerce_unsigned(), 7);
    }

    #[test]
    fn server_message_2_config_attrs() {
        let mut mgr = Manager::without_files(None);
        let cfg = Configuration_Optional {
            congestion: Some(CongestionController::Bbr),
            initial_congestion_window: Some(42),
            timeout: Some(88),
            ..Default::default()
        };
        mgr.merge_provider(&cfg);
        let final_cfg = mgr.get::<Configuration>().unwrap();
        let mut msg = ServerMessageV2::default();
        msg.apply_config_attributes(&final_cfg);

        let attrs = &msg.attributes;

        let tag = attrs
            .find_tag(ServerMessage2Attributes::CongestionController)
            .unwrap();
        assert_eq!(tag.coerce_unsigned(), CongestionController::Bbr as u64);

        let tag = attrs
            .find_tag(ServerMessage2Attributes::InitialCongestionWindow)
            .unwrap();
        assert_eq!(tag.coerce_unsigned(), 42);

        let tag = attrs
            .find_tag(ServerMessage2Attributes::QuicTimeout)
            .unwrap();
        assert_eq!(tag.coerce_unsigned(), 88);
    }

    #[test]
    fn server_message_2_to_provider() {
        let msg = ServerMessageV2 {
            bandwidth_to_server: Uint(12345),
            bandwidth_to_client: Uint(54321),
            rtt: 42,
            attributes: vec![
                ServerMessage2Attributes::CongestionController
                    .with_unsigned(CongestionController::Bbr as u64),
                ServerMessage2Attributes::InitialCongestionWindow.with_unsigned(5544u32),
                ServerMessage2Attributes::QuicTimeout.with_unsigned(55u32),
                // these two are not part of the config:
                ServerMessage2Attributes::WarningMessage.with_str("hi"),
                ServerMessage2Attributes::Invalid.into(),
            ],
            ..Default::default()
        };
        let mut mgr = Manager::without_files(None);
        mgr.apply_system_default();
        mgr.merge_provider(msg);
        let cfg = mgr.get::<Configuration>().unwrap();
        assert_eq!(cfg.rx, 54321);
        assert_eq!(cfg.tx, 12345);
        assert_eq!(cfg.rtt, 42);
        assert_eq!(cfg.congestion, CongestionController::Bbr);
        assert_eq!(cfg.initial_congestion_window, 5544);
        assert_eq!(cfg.timeout, 55);
    }

    #[test]
    fn server_message_2_display() {
        let msg = ServerMessageV2 {
            bandwidth_to_client: Uint(12_000),
            bandwidth_to_server: Uint(1_987_654_321),
            ..Default::default()
        };
        let s = format!("{msg}");
        eprintln!("{s}");
        assert_contains!(s, "in 1.987G");
        assert_contains!(s, "out 12k");
    }
}
