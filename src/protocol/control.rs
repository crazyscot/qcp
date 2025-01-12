//! Control protocol definitions and helper types
// (c) 2024 Ross Younger
//!
//! The control protocol consists data passed between the local qcp client process and the remote qcp server process
//! before establishing the [QUIC] connection.
//! The two processes are connected via ssh.
//!
//! The control protocol looks like this:
//! * Server ➡️ Client: Banner
//! * C ➡️ S: [`ClientMessage`]
//! * S ➡️ C: [`ServerMessage`]
//! * Client establishes a QUIC connection to the server, on the port given in the [`ServerMessage`].
//! * Client then opens one or more bidirectional QUIC streams ('sessions') on that connection.
//!    (See the session protocol for what happens there.)
//!
//! When transfer is complete and all QUIC streams are closed:
//! * S ➡️ C: [`ClosedownReport`]
//! * C ➡️ S: (closes control channel; server takes this as a cue to exit)
//!
//! On the wire these are [BARE] messages, sent using standard framing.
//!
//! # See also
//! [Common](super::common) protocol functions
//!
//! [quic]: https://quicwg.github.io/
//! [BARE]: https://www.ietf.org/archive/id/draft-devault-bare-11.html

use std::net::IpAddr;

use quinn::ConnectionStats;
use serde::{Deserialize, Serialize};
use serde_bare::Uint;

use super::common::ProtocolMessage;

/// Server banner message, sent on stdout and checked by the client
pub const BANNER: &str = "qcp-server-2\n";

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
#[derive(Clone, Copy, Debug, strum::Display, PartialEq, Eq, strum::FromRepr)]
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

impl CompatibilityLevel {
    /// Returns the underlying u16 representation of this value
    #[must_use]
    pub fn discriminant(self) -> u16 {
        #[allow(unsafe_code)]
        // SAFETY: As this type is marked `repr(u16)`, its contents are guaranteed to lie in the range 0..65535.
        unsafe {
            std::mem::transmute(self)
        }
    }
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
        value.discriminant()
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Clone, Copy)]
/// Protocol representation of a connection type
///
/// Unlike [`AddressFamily`](crate::util::AddressFamily) there is no ANY; types must be explicit here.
#[repr(u8)]
pub enum ConnectionType {
    /// IP version 4
    #[default]
    Ipv4 = 4,
    /// IP version 6
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

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// The initial message from client to server.
///
/// This is mildly tricky, as we have to send it without knowing what version the server supports.
pub struct ClientMessage {
    /// Client's self-signed certificate (DER)
    pub cert: Vec<u8>,
    /// The connection type to use (the type of socket we want the server to bind)
    pub connection_type: ConnectionType,
    /// Protocol compatibility version identifier
    ///
    /// This identifies the client's maximum supported protocol sub-version.
    ///
    /// N.B. This is not sent as an enum to avoid breaking the server when we have a newer version!
    pub compatibility: u16,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}
impl ProtocolMessage for ClientMessage {}

#[derive(Serialize, Default, Deserialize, PartialEq, Eq, Debug)]
#[repr(u16)]
/// The initial message from client to server
pub enum ServerMessage {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    V1(ServerMessageV1) = 1,

    /// Special value indicating a client has not yet read the remote `ServerMessage`.
    /// This value should never be seen on the wire.
    #[default]
    ToFollow = 0,
}
impl ProtocolMessage for ServerMessage {}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default)]
/// Version 1 of the message from server to client.
/// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
pub struct ServerMessageV1 {
    /// Protocol compatibility version identifier
    ///
    /// This identifies the server's maximum supported protocol sub-version.
    ///
    /// N.B. This is not sent as an enum to avoid breaking the client when we have a newer version!
    pub compatibility: u16,

    /// UDP port the server has bound to
    pub port: u16,
    /// Server's self-signed certificate (DER)
    pub cert: Vec<u8>,
    /// Name in the server cert (this saves us having to unpick it from the certificate)
    pub name: String,
    /// If non-zero length, this is a warning message to be relayed to a human
    pub warning: String,
    /// Reports the server's active bandwidth configuration
    pub bandwidth_info: String,
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Copy, Clone)]
#[repr(u16)]
/// The statistics sent by the server when the job is done
pub enum ClosedownReport {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    V1(ClosedownReportV1) = 1,
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

    use crate::protocol::{
        common::ProtocolMessage,
        control::{CompatibilityLevel, ConnectionType},
    };

    use super::ClosedownReportV1;

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
}
