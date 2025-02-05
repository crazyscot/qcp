//! # Control protocol definitions and helper types
// (c) 2024 Ross Younger
//!
//! The control protocol consists of data passed between the local qcp client process and the remote qcp server process
//! before establishing the [QUIC] connection.
//! The two processes are connected via ssh.
//!
//! The control protocol looks like this:
//! * Server ➡️ Client: Banner
//! * C ➡️ S: [`ClientGreeting`]
//! * S ➡️ C: [`ServerGreeting`]
//!   * The two greetings may be sent in parallel.
//! * C ➡️ S: [`ClientMessage`]
//!   * The client MUST NOT send its Message until it has received the `ServerGreeting`,
//!     and it MUST NOT send a newer version of the `ClientMessage` than the server understands.
//! * S: ⚙️ Parses client message, applies parameter negotiation rules
//!     (see [`combine_bandwidth_configurations`](crate::transport::combine_bandwidth_configurations)),
//!     binds to a UDP port for the session protocol.
//! * S ➡️ C: [`ServerMessage`]
//!   * The server MUST NOT send a newer version of the `ServerMessage` than the client understands.
//! * Client establishes a QUIC connection to the server, on the port given in the [`ServerMessage`].
//! * Client then opens one or more bidirectional QUIC streams ('sessions') on that connection.
//!    (See the [session protocol](crate::protocol::session) for what happens there.)
//!
//! When transfer is complete and all QUIC streams are closed:
//! * S ➡️ C: [`ClosedownReport`]
//!   * The server MUST NOT send a newer version than the client understands.
//! * C ➡️ S: (closes control channel; server takes this as a cue to exit)
//!
//! On the wire these are [BARE] messages.
//!
//! # See also
//! [Common](super::common) protocol functions
//!
//! [quic]: https://quicwg.github.io/
//! [BARE]: https://www.ietf.org/archive/id/draft-devault-bare-11.html

use std::{fmt::Display, net::IpAddr};

use anyhow::anyhow;
use figment::{
    value::{Dict, Map},
    Profile, Provider,
};
use quinn::ConnectionStats;
use serde::{Deserialize, Serialize};
use serde_bare::Uint;
use serde_repr::{Deserialize_repr, Serialize_repr};

use super::common::ProtocolMessage;
use crate::{
    config::{Configuration_Optional, Manager},
    transport::CongestionControllerType,
    util::{insert_if_some, Credentials, PortRange as CliPortRange},
};

/// Server banner message, sent on stdout and checked by the client
pub const BANNER: &str = "qcp-server-2\n";

/// The banner for the initial protocol version (pre-v0.3) that we don't support any more.
/// Note that it is the same size as the current [`BANNER`].
pub const OLD_BANNER: &str = "qcp-server-1\n";

/// The protocol compatibility version implemented by this crate
pub const COMPATIBILITY_LEVEL: CompatibilityLevel = CompatibilityLevel::V1;

////////////////////////////////////////////////////////////////////////////////////////
// COMPATIBILITY

/// Protocol sub-version compatibility identifier
///
/// This forms part of the negotiation between client and server.
/// An endpoint declares the highest version of the protocol that it understands.
///
/// An endpoint MUST NOT send any structure variants newer than its peer understands.
///
/// While this enum is part of the control protocol, it affects both control and session; the same principles
/// of compatibility apply.
#[repr(u16)]
#[derive(Clone, Copy, Debug, strum::Display, PartialEq, Eq, strum::FromRepr, PartialOrd, Ord)]
pub enum CompatibilityLevel {
    /// Indicates that we do not know the peer's compatibility level.
    /// This value should never be seen on the wire.
    UNKNOWN = 0,

    /// Version 1 was introduced in qcp 0.3
    V1 = 1,

    /// Special value indicating the peer is newer than our latest version.
    /// This value should never be seen on the wire.
    /// Where the peer is `NEWER` than us, we would expect to use the latest protocol version we know about.
    NEWER = 65535,
}

impl From<u16> for CompatibilityLevel {
    /// This conversion is infallible because any unknown value is mapped to `VersionCompatibility::NEWER`.
    fn from(value: u16) -> Self {
        match CompatibilityLevel::from_repr(value) {
            Some(v) => v,
            None => CompatibilityLevel::NEWER,
        }
    }
}
impl From<CompatibilityLevel> for u16 {
    fn from(value: CompatibilityLevel) -> Self {
        value as u16
    }
}

////////////////////////////////////////////////////////////////////////////////////////
// CONNECTION TYPE

#[derive(
    Serialize_repr, Deserialize_repr, PartialEq, Eq, Debug, Default, Clone, Copy, strum::Display,
)]
/// Protocol representation of a connection type
///
/// Unlike [`AddressFamily`](crate::util::AddressFamily) there is no ANY; types must be explicit here.
#[repr(u8)]
pub enum ConnectionType {
    /// IP version 4 (serialize as the byte 0x04)
    #[default]
    Ipv4 = 4,
    /// IP version 6 (serialize as the byte 0x06)
    Ipv6 = 6,
}

impl From<IpAddr> for ConnectionType {
    fn from(value: IpAddr) -> Self {
        match value {
            IpAddr::V4(_) => ConnectionType::Ipv4,
            IpAddr::V6(_) => ConnectionType::Ipv6,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////
// CONGESTION CONTROLLER

/// Selects the congestion control algorithm to use.
/// This is a newtype for [crate::transport::CongestionControllerType]
/// with different serialization.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Clone, Copy)]
#[serde(try_from = "Uint")]
#[serde(into = "Uint")]
#[allow(non_camel_case_types)]
pub struct CongestionController_OnWire(pub CongestionControllerType);

impl From<CongestionController_OnWire> for CongestionControllerType {
    fn from(value: CongestionController_OnWire) -> Self {
        value.0
    }
}
impl From<CongestionControllerType> for CongestionController_OnWire {
    fn from(value: CongestionControllerType) -> Self {
        Self(value)
    }
}

impl From<CongestionController_OnWire> for Uint {
    fn from(value: CongestionController_OnWire) -> Self {
        Self(value.0 as u64)
    }
}

impl TryFrom<Uint> for CongestionController_OnWire {
    type Error = anyhow::Error;

    fn try_from(value: Uint) -> anyhow::Result<Self> {
        let v = u8::try_from(value.0)?;
        let t = CongestionControllerType::from_repr(v)
            .ok_or(anyhow!("invalid congestioncontroller enum"))?;
        Ok(Self(t))
    }
}

impl Display for CongestionController_OnWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

////////////////////////////////////////////////////////////////////////////////////////
// PORT RANGE

/// Representation of a TCP or UDP port range
///
/// N.B. This type is structurally identical to, but distinct from,
/// [`crate::util::PortRange`] so that it can have different serialization
/// semantics.
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
#[allow(non_camel_case_types)]
pub struct PortRange_OnWire {
    /// The first port of the range
    pub begin: u16,
    /// The last port of the range, inclusive. This may be the same as the first.
    pub end: u16,
}

impl From<CliPortRange> for PortRange_OnWire {
    fn from(other: CliPortRange) -> Self {
        Self {
            begin: other.begin,
            end: other.end,
        }
    }
}

impl From<CliPortRange> for Option<PortRange_OnWire> {
    fn from(value: CliPortRange) -> Self {
        if value.is_default() {
            None
        } else {
            Some(value.into())
        }
    }
}

impl From<PortRange_OnWire> for CliPortRange {
    fn from(other: PortRange_OnWire) -> Self {
        Self {
            begin: other.begin,
            end: other.end,
        }
    }
}

impl Display for PortRange_OnWire {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}-{}", self.begin, self.end)
    }
}

////////////////////////////////////////////////////////////////////////////////////////
// CLIENT GREETING

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// The initial message from client to server.
///
/// We have to send this message without knowing what version the server supports.
pub struct ClientGreeting {
    /// Protocol compatibility version identifier
    ///
    /// This identifies the client's maximum supported protocol sub-version.
    ///
    /// N.B. This is not sent as an enum to avoid breaking the server when we have a newer version!
    pub compatibility: u16,
    /// Requests the remote emit debug information over the control channel (stderr).
    pub debug: bool,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}
impl ProtocolMessage for ClientGreeting {}

////////////////////////////////////////////////////////////////////////////////////////
// SERVER GREETING

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// The initial message from server to client.
///
/// Like [`ClientGreeting`] this is designed to be sent without knowing what version the client supports.
pub struct ServerGreeting {
    /// Protocol compatibility version identifier
    ///
    /// This identifies the client's maximum supported protocol sub-version.
    ///
    /// N.B. This is not sent as an enum to avoid breaking the server when we have a newer version!
    pub compatibility: u16,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}
impl ProtocolMessage for ServerGreeting {}

////////////////////////////////////////////////////////////////////////////////////////
// CLIENT MESSAGE

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default, derive_more::Display)]
/// The control parameters send from client to server.
pub enum ClientMessage {
    /// Special value indicating an endpoint has not yet read the remote `ClientMessage`.
    /// This value should never be seen on the wire.
    #[default]
    #[serde(skip_serializing)]
    ToFollow, // 0
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    /// On the wire this is encoded with enum discriminant 1.
    V1(ClientMessageV1), //
}
impl ProtocolMessage for ClientMessage {}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// Version 1 of the client control parameters message.
/// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
pub struct ClientMessageV1 {
    /// Client's self-signed certificate (DER)
    pub cert: Vec<u8>,
    /// The connection type to use (the type of socket we want the server to bind)
    pub connection_type: ConnectionType,
    /// If present, requests the server bind to a UDP port from a given range.
    pub port: Option<PortRange_OnWire>,

    /// The requested bandwidth to use from client to server
    pub bandwidth_to_server: Option<Uint>,
    /// The requested bandwidth to use from server to client (if None, use the same as bandwidth to server)
    pub bandwidth_to_client: Option<Uint>,
    /// The network Round Trip Time, in milliseconds, to use in calculating the bandwidth delay product
    pub rtt: Option<u16>,
    /// The congestion control algorithm to use
    pub congestion: Option<CongestionController_OnWire>,
    /// The initial congestion window, if specified
    pub initial_congestion_window: Option<Uint>,
    /// Connection timeout for the QUIC endpoints, in seconds
    pub timeout: Option<u16>,

    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}

impl ClientMessage {
    pub(crate) fn new(
        credentials: &Credentials,
        connection_type: ConnectionType,
        manager: &Manager,
    ) -> Self {
        ClientMessage::V1(ClientMessageV1::new(credentials, connection_type, manager))
    }
}

impl ClientMessageV1 {
    fn new(credentials: &Credentials, connection_type: ConnectionType, manager: &Manager) -> Self {
        let working = manager.get::<Configuration_Optional>().unwrap_or_default();

        Self {
            cert: credentials.certificate.to_vec(),
            connection_type,
            port: manager
                .get_field_optional::<CliPortRange>("remote_port")
                .map(std::convert::Into::into),

            bandwidth_to_server: working.tx.map(u64::from).map(Uint),
            bandwidth_to_client: working.rx.map(u64::from).map(Uint),
            rtt: working.rtt,
            congestion: working.congestion.map(CongestionController_OnWire),
            initial_congestion_window: working.initial_congestion_window.map(Uint),
            timeout: working.timeout,

            extension: 0,
        }
    }

    const META_NAME: &str = "client message";
}

impl Provider for ClientMessageV1 {
    fn metadata(&self) -> figment::Metadata {
        figment::Metadata::named(Self::META_NAME)
    }

    fn data(&self) -> Result<figment::value::Map<figment::Profile, Dict>, figment::Error> {
        let mut dict = Dict::new();

        // This is written from the consumer's (server's) point of view, i.e. bandwidth_to_server is server's rx.
        // N.B. storing tx & rx as integers here requires engineering_repr 1.1.0.
        insert_if_some(&mut dict, "rx", self.bandwidth_to_server.map(|v| v.0))?;
        insert_if_some(&mut dict, "tx", self.bandwidth_to_client.map(|v| v.0))?;
        insert_if_some(&mut dict, "rtt", self.rtt)?;
        insert_if_some(&mut dict, "congestion", self.congestion)?;
        insert_if_some(&mut dict, "timeout", self.timeout)?;
        insert_if_some(
            &mut dict,
            "initial_congestion_window",
            self.initial_congestion_window,
        )?;

        let mut profile_map = Map::new();
        let _ = profile_map.insert(Profile::Global, dict);

        Ok(profile_map)
    }
}

impl Display for ClientMessageV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "type {}, port {:?}, ToClient {:?}, ToServer {:?}, rtt {:?}, congestion {:?}/{:?}, timeout {:?}",
            self.connection_type,
            self.port,
            self.bandwidth_to_client,
            self.bandwidth_to_server,
            self.rtt,
            self.congestion,
            self.initial_congestion_window,
            self.timeout
        )
    }
}

////////////////////////////////////////////////////////////////////////////////////////
// SERVER MESSAGE

#[derive(Clone, Serialize, Default, Deserialize, PartialEq, Eq, Debug)]
/// The control parameters sent from server to client
pub enum ServerMessage {
    /// Special value indicating an endpoint has not yet read the remote `ServerMessage`.
    /// This value should never be seen on the wire.
    #[default]
    #[serde(skip_serializing)]
    ToFollow,
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    /// On the wire enum discriminant: 1.
    V1(ServerMessageV1),
}
impl ProtocolMessage for ServerMessage {}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// Version 1 of the message from server to client.
/// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
pub struct ServerMessageV1 {
    /// UDP port the server has bound to
    pub port: u16,
    /// Server's self-signed certificate (DER)
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
    pub congestion: CongestionController_OnWire,
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
        insert("congestion", self.congestion.0.to_string().into());
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

////////////////////////////////////////////////////////////////////////////////////////
// CLOSEDOWN REPORT

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Copy, Clone)]
/// The statistics sent by the server when the job is done
pub enum ClosedownReport {
    /// Special value that should never be seen on the wire
    #[serde(skip_serializing)]
    Unknown,
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    /// On the wire enum discriminant: 1
    V1(ClosedownReportV1),
}
impl ProtocolMessage for ClosedownReport {}

/// Version 1 of the closedown report.
/// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Copy, Clone)]
pub struct ClosedownReportV1 {
    /// Final congestion window
    pub cwnd: Uint,
    /// Number of packets sent
    pub sent_packets: Uint,
    /// Number of packets lost
    pub lost_packets: Uint,
    /// Number of bytes lost
    pub lost_bytes: Uint,
    /// Number of congestion events detected
    pub congestion_events: Uint,
    /// Number of black holes detected
    pub black_holes: Uint,
    /// Number of bytes sent
    pub sent_bytes: Uint,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}

impl From<&ConnectionStats> for ClosedownReportV1 {
    fn from(stats: &ConnectionStats) -> Self {
        let ps = &stats.path;
        Self {
            cwnd: Uint(ps.cwnd),
            sent_packets: Uint(ps.sent_packets),
            sent_bytes: Uint(stats.udp_tx.bytes),
            lost_packets: Uint(ps.lost_packets),
            lost_bytes: Uint(ps.lost_bytes),
            congestion_events: Uint(ps.congestion_events),
            black_holes: Uint(ps.black_holes_detected),
            extension: 0,
        }
    }
}

#[cfg(test)]
mod test {
    use std::{
        io::Cursor,
        net::{IpAddr, Ipv4Addr, Ipv6Addr},
    };

    use quinn::ConnectionStats;
    use serde::{Deserialize, Serialize};
    use serde_bare::Uint;

    use crate::{
        config::{Configuration_Optional, Manager},
        protocol::{
            common::ProtocolMessage,
            control::{
                ClosedownReport, CompatibilityLevel, CongestionController_OnWire, ConnectionType,
                ServerGreeting, ServerMessageV1,
            },
        },
        transport::CongestionControllerType,
        util::Credentials,
    };

    use super::{ClientGreeting, ClientMessage, ClosedownReportV1, ServerMessage};

    #[test]
    fn test_connection_type() {
        let ip4 = IpAddr::from(Ipv4Addr::LOCALHOST);
        let ct4 = ConnectionType::from(ip4);
        assert_eq!(ct4, ConnectionType::Ipv4);

        let ip6 = IpAddr::from(Ipv6Addr::LOCALHOST);
        let ct6 = ConnectionType::from(ip6);
        assert_eq!(ct6, ConnectionType::Ipv6);
    }

    #[test]
    fn test_closedown_report() {
        use serde_bare::Uint;

        let mut stats = ConnectionStats::default();
        stats.path.cwnd = 42;
        stats.path.black_holes_detected = 88;
        stats.udp_tx.bytes = 12345;
        let report = ClosedownReportV1::from(&stats);
        let expected = ClosedownReportV1 {
            cwnd: Uint(42),
            black_holes: Uint(88),
            sent_bytes: Uint(12345),
            ..Default::default()
        };
        assert_eq!(report, expected);
    }

    #[test]
    fn convert_version_compat() {
        assert_eq!(u16::from(CompatibilityLevel::V1), 1);
        assert_eq!(CompatibilityLevel::from(1), CompatibilityLevel::V1);
        assert_eq!(CompatibilityLevel::from(12345), CompatibilityLevel::NEWER);
    }

    /// Time-travelling compatibility: Version 1 of the structure.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Test1 {
        i: i32,
        /// In v2 this is an Optional member. In v1 we simply encode as zero, which is interpreted as an Option that is not present.
        extension: u8,
    }
    impl ProtocolMessage for Test1 {}

    /// Time-travelling compatibility: Version 2 of the structure
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct Test2 {
        i: i32,
        // In v1 this is a u8 sent as zero.
        whatever: Option<u64>,
    }
    impl ProtocolMessage for Test2 {}

    #[test]
    /// Confirms that the "extension: u8" trick works, forwards through time.
    /// That is to say, we can encode V1 and decode it as V2.
    fn forwards_compatibility() {
        let t1 = Test1 {
            i: 42,
            extension: 0,
        };
        let mut buf = Vec::<u8>::new();
        t1.to_writer_framed(&mut buf).unwrap();

        let decoded = Test2::from_reader_framed(&mut Cursor::new(buf)).unwrap();
        // The real test here is that decode succeeded.
        assert_eq!(decoded.i, t1.i);
        assert!(decoded.whatever.is_none());
    }

    #[test]
    /// Confirms that the "extension: u8" trick works, backwards through time.
    /// That is to say, we can encode V2 of the structure and decode it as V1 (without its optional fields).
    fn backwards_compatibility() {
        let t2 = Test2 {
            i: 78,
            whatever: Some(12345),
        };
        let mut buf = Vec::<u8>::new();
        t2.to_writer_framed(&mut buf).unwrap();

        let decoded = Test1::from_reader_framed(&mut Cursor::new(buf)).unwrap();
        // The real test here is that decode succeeded.
        assert_eq!(decoded.i, t2.i);
        assert_eq!(decoded.extension, 1);
    }

    #[test]
    fn serialize_client_greeting() {
        let msg = ClientGreeting {
            compatibility: CompatibilityLevel::V1.into(),
            debug: false,
            extension: 0,
        };
        let wire = msg.to_vec().unwrap();
        let deser = ClientGreeting::from_slice(&wire).unwrap();
        assert_eq!(msg, deser);
    }

    #[test]
    fn serialize_server_greeting() {
        let msg = ServerGreeting {
            compatibility: CompatibilityLevel::V1.into(),
            extension: 0,
        };
        let wire = msg.to_vec().unwrap();
        let deser = ServerGreeting::from_slice(&wire).unwrap();
        assert_eq!(msg, deser);
    }

    #[test]
    fn serialize_client_message() {
        use engineering_repr::EngineeringQuantity as EQ;

        let fake_keypair = &[0u8];
        let keypair = rustls_pki_types::PrivatePkcs8KeyDer::<'_>::from(fake_keypair.as_slice());
        let creds = Credentials {
            certificate: vec![0, 1, 2].into(),
            keypair: keypair.into(),
            hostname: "foo".into(),
        };
        let mut manager = Manager::without_files(None);
        let config = Configuration_Optional {
            tx: Some(42u64.into()),
            rx: Some(89u64.into()),
            rtt: Some(1234),
            congestion: Some(crate::transport::CongestionControllerType::Bbr),
            ..Default::default()
        };
        manager.merge_provider(&config);
        let _ = manager.get_field::<EQ<u64>>("tx").unwrap();
        assert!(manager.get_field_optional::<EQ<u64>>("tx").is_some());
        assert_eq!(
            manager.get_field_optional::<EQ<u64>>("tx").unwrap(),
            42u64.into()
        );
        assert_eq!(
            manager
                .get_field::<CongestionControllerType>("congestion")
                .unwrap(),
            CongestionControllerType::Bbr
        );
        assert_eq!(manager.get_field::<u16>("rtt").unwrap(), 1234);

        let cmsg = ClientMessage::new(&creds, ConnectionType::Ipv4, &manager);
        let ser = cmsg.to_vec().unwrap();
        //println!("{cmsg:#?}");
        //println!("vec: {ser:?}");
        let deser = ClientMessage::from_slice(&ser).unwrap();
        println!("{deser:#?}");
        if let ClientMessage::V1(detail) = &deser {
            assert_eq!(detail.bandwidth_to_server.unwrap().0, 42);
            assert_eq!(detail.bandwidth_to_client.unwrap().0, 89);
            assert_eq!(
                detail.congestion,
                Some(CongestionController_OnWire(CongestionControllerType::Bbr))
            );
        } else {
            panic!("wrong ClientMessage type");
        }
        assert_eq!(cmsg, deser);
    }

    #[test]
    fn serialize_server_message() {
        let msg = ServerMessage::V1(ServerMessageV1 {
            port: 12345,
            cert: vec![9, 8, 7],
            name: "hello".to_string(),
            bandwidth_to_client: Uint(123),
            bandwidth_to_server: Uint(456),
            rtt: 789,
            congestion: CongestionController_OnWire(CongestionControllerType::Bbr),
            initial_congestion_window: Uint(4321),
            timeout: 42,
            warning: String::from("this is a warning"),
            extension: 0,
        });
        let wire = msg.to_vec().unwrap();
        let deser = ServerMessage::from_slice(&wire).unwrap();
        assert_eq!(msg, deser);
    }

    #[test]
    fn serialize_closedown_report() {
        let msg = ClosedownReport::V1(ClosedownReportV1 {
            cwnd: Uint(42),
            sent_packets: Uint(123),
            lost_packets: Uint(234),
            lost_bytes: Uint(456_798),
            congestion_events: Uint(44),
            black_holes: Uint(22),
            sent_bytes: Uint(987_654),
            extension: 0,
        });
        let wire = msg.to_vec().unwrap();
        let deser = ClosedownReport::from_slice(&wire).unwrap();
        assert_eq!(msg, deser);
    }

    #[test]
    fn skip_serializing() {
        let msg = ServerMessage::ToFollow;
        let _ = msg.to_vec().expect_err("ToFollow cannot be serialized");
    }
}
