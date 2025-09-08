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
//!   (see [`combine_bandwidth_configurations`](crate::transport::combine_bandwidth_configurations)),
//!   binds to a UDP port for the session protocol.
//! * S ➡️ C: [`ServerMessage`]
//!   * The server MUST NOT send a newer version of the `ServerMessage` than the client understands.
//! * Client establishes a QUIC connection to the server, on the port given in the [`ServerMessage`].
//! * Client then opens one or more bidirectional QUIC streams ('sessions') on that connection.
//!   (See the [session protocol](crate::protocol::session) for what happens there.)
//!
//! When transfer is complete and all QUIC streams are closed:
//! * S ➡️ C: [`ClosedownReport`]
//!   * The server MUST NOT send a newer version than the client understands.
//! * C ➡️ S: (closes control channel; server takes this as a cue to exit)
//!
//! # Wire encoding
//!
//! On the wire these are [BARE] messages.
//!
//! Note that serde_bare by default encodes enums on the wire as uints (rust `usize`),
//! ignoring any explicit discriminant!
//!
//! Unit enums (C-like) may be encoded with explicitly sized types (repr attribute) and using
//! their discriminant as the wire value, if derived from `Serialize_repr` or `Deserialize_repr`.
//!
//! # See also
//! [Common](super::common) protocol functions
//!
//! [quic]: https://quicwg.github.io/
//! [BARE]: https://www.ietf.org/archive/id/draft-devault-bare-11.html

use std::net::{IpAddr, SocketAddr};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_bare::Uint;
use serde_repr::{Deserialize_repr, Serialize_repr};

use super::common::ProtocolMessage;
use crate::{protocol::display_vec_td, util::PortRange as CliPortRange};

/// Server banner message, sent on stdout and checked by the client
pub const BANNER: &str = "qcp-server-2\n";

/// The banner for the initial protocol version (pre-v0.3) that we don't support any more.
/// Note that it is the same size as the current [`BANNER`].
pub const OLD_BANNER: &str = "qcp-server-1\n";

/// The protocol compatibility version implemented by this crate
pub(crate) const OUR_COMPATIBILITY_NUMERIC: u16 = 3;
/// The protocol compatibility version implemented by this crate
pub const OUR_COMPATIBILITY_LEVEL: Compatibility = Compatibility::Level(OUR_COMPATIBILITY_NUMERIC);

mod client_msg;
pub use client_msg::*;

mod server_msg;
pub use server_msg::*;

mod greetings;
pub use greetings::*;

mod closedown;
pub use closedown::*;

////////////////////////////////////////////////////////////////////////////////////////
// Display helpers

use engineering_repr::EngineeringQuantity as EQ;

fn display_opt_uint(label: &str, bandwidth: Option<&Uint>) -> String {
    bandwidth.map_or_else(String::new, |u| {
        format!(", {label}: {}", EQ::<u64>::from(u.0))
    })
}

fn display_opt<T: std::fmt::Display>(label: &str, value: Option<&T>) -> String {
    value
        .as_ref()
        .map_or_else(String::new, |v| format!(", {label}: {v}"))
}

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
///
/// The following compatibility levels are defined:
/// * 1: Introduced in qcp 0.3.
/// * 2: Introduced in qcp 0.5.
///
/// See [`crate::protocol::compat::Feature`] for a mapping from compatibility levels to specific features.
///
/// <div class="warning">
/// While this type implements an automatic `PartialEq`, it does not offer an `Ord` or `PartialOrd`
/// due to the special meanings of [`CompatibilityLevel::Unknown`] and [`CompatibilityLevel::Newer`].
/// Prefer to use a match block and compare the u16 within directly.
/// </div>
///
#[derive(Clone, Copy, Debug, Default, derive_more::Display, PartialEq, Serialize, Deserialize)]
pub enum Compatibility {
    /// Indicates that we do not (yet) know the peer's compatibility level.
    ///
    /// This value should never be seen on the wire. The set of supported features is undefined.
    ///
    /// This value is not considered to be equal to itself. Use a match block if you need to test for unknown-ness.
    #[default]
    #[serde(skip_serializing)]
    Unknown,
    /// Special value indicating the peer is newer than the latest version we now about.
    ///
    /// This value should never be seen on the wire.
    /// The set of supported features is assumed to be an unspecified superset of ours.
    ///
    /// Where the peer is `Newer` than us, we would expect to use the latest protocol version we know about.
    ///
    #[serde(skip_serializing)]
    Newer,

    /// General compatibility level, serialized as a u16.
    #[serde(untagged)]
    Level(u16),
}

impl From<Compatibility> for u16 {
    fn from(value: Compatibility) -> Self {
        match value {
            Compatibility::Level(v) => v,
            Compatibility::Unknown | Compatibility::Newer => 0,
        }
    }
}

impl From<u16> for Compatibility {
    fn from(value: u16) -> Self {
        if value > OUR_COMPATIBILITY_NUMERIC {
            // If the value is greater than our compatibility level, we treat it as "newer"
            Compatibility::Newer
        } else {
            Compatibility::Level(value)
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////
// CONNECTION TYPE

#[derive(
    Serialize_repr,
    Deserialize_repr,
    PartialEq,
    Eq,
    Debug,
    Default,
    Clone,
    Copy,
    strum_macros::Display,
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

impl From<SocketAddr> for ConnectionType {
    fn from(value: SocketAddr) -> Self {
        match value {
            SocketAddr::V4(_) => ConnectionType::Ipv4,
            SocketAddr::V6(_) => ConnectionType::Ipv6,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////
// CONGESTION CONTROLLER

/// Selects the congestion control algorithm to use.
/// This structure is serialized as a standard BARE enum.
/// To serialize it as a string, see [`crate::util::SerializeAsString`].
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    strum_macros::Display,
    strum_macros::EnumString,
    strum_macros::FromRepr,
    strum_macros::VariantNames,
    clap::ValueEnum,
)]
#[serde(try_from = "Uint")]
#[serde(into = "Uint")]
#[strum(serialize_all = "lowercase")] // N.B. this applies to EnumString, not Display
pub enum CongestionController {
    /// The congestion algorithm TCP uses. This is good for most cases.
    //
    // Note that this enum is serialized without serde_repr, so explicit discriminants are not used on the wire.
    // This also means that the ordering and meaning can never be changed without breaking compatibility.
    #[default]
    Cubic,
    /// (Use with caution!) An experimental algorithm created by Google,
    /// which increases goodput in some situations
    /// (particularly long and fat connections where the intervening
    /// buffers are shallow). However this comes at the cost of having
    /// more data in-flight, and much greater packet retransmission.
    /// See
    /// `https://blog.apnic.net/2020/01/10/when-to-use-and-not-use-bbr/`
    /// for more discussion.
    Bbr,
    /// The traditional "NewReno" congestion algorithm.
    /// This was the algorithm used in TCP before the introduction of Cubic.
    ///
    /// This option requires qcp protocol compatibility level V2.
    NewReno,
}

impl From<CongestionController> for Uint {
    fn from(value: CongestionController) -> Self {
        Self(value as u64)
    }
}

impl TryFrom<Uint> for CongestionController {
    type Error = anyhow::Error;

    fn try_from(value: Uint) -> anyhow::Result<Self> {
        let v = usize::try_from(value.0)?;
        CongestionController::from_repr(v).ok_or(anyhow!("invalid congestioncontroller enum"))
    }
}

impl From<CongestionController> for figment::value::Value {
    fn from(value: CongestionController) -> Self {
        value.to_string().into()
    }
}

////////////////////////////////////////////////////////////////////////////////////////
// PORT RANGE

/// Representation of a TCP or UDP port range
///
/// N.B. This type is structurally identical to, but distinct from,
/// [`crate::util::PortRange`] so that it can have different serialization
/// semantics.
#[derive(
    Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Debug, Default, derive_more::Display,
)]
#[allow(non_camel_case_types)]
#[display("{}-{}", begin, end)]
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

// //////////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use std::{
        io::Cursor,
        net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    };

    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};

    use crate::{
        protocol::{
            DataTag as _, TaggedData,
            common::ProtocolMessage,
            control::{Compatibility, CongestionController, ConnectionType, CredentialsType},
        },
        util::{PortRange as CliPortRange, serialization::SerializeAsString},
    };

    use super::PortRange_OnWire;

    // helper function - creates a bogus certificate
    pub(crate) fn dummy_cert() -> Vec<u8> {
        vec![0, 1, 2]
    }
    // helper function - creates a bogus Credentials
    pub(crate) fn dummy_credentials() -> TaggedData<CredentialsType> {
        CredentialsType::X509.with_bytes(vec![0, 1, 2])
    }

    #[test]
    fn convert_connection_type() {
        let ip4 = IpAddr::from(Ipv4Addr::LOCALHOST);
        let ct4 = ConnectionType::from(ip4);
        assert_eq!(ct4, ConnectionType::Ipv4);

        let ip6 = IpAddr::from(Ipv6Addr::LOCALHOST);
        let ct6 = ConnectionType::from(ip6);
        assert_eq!(ct6, ConnectionType::Ipv6);

        let sa4: SocketAddr = "127.0.0.1:1234".parse().unwrap();
        let ct4 = ConnectionType::from(sa4);
        assert_eq!(ct4, ConnectionType::Ipv4);

        let sa6: SocketAddr = "[::1]:4321".parse().unwrap();
        let ct6 = ConnectionType::from(sa6);
        assert_eq!(ct6, ConnectionType::Ipv6);
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
    fn type_conversions_congestion() {
        let c = CongestionController::Cubic;
        let c2 = SerializeAsString::<CongestionController>::from(c);
        println!("{c2}");
        assert_eq!(*c2, c);
    }

    #[test]
    fn type_conversions_port_range() {
        let cli = CliPortRange { begin: 1, end: 10 };
        let wire = PortRange_OnWire::from(cli);
        assert_eq!(CliPortRange::from(wire), cli);
        println!("{wire}");

        let opt1: Option<PortRange_OnWire> = cli.into();
        assert_eq!(opt1, Some(PortRange_OnWire { begin: 1, end: 10 }));
        let opt2: Option<PortRange_OnWire> = CliPortRange::default().into();
        assert_eq!(opt2, None);
    }

    #[test]
    fn compat_level_from_wire() {
        let cases = &[
            (0u16, Compatibility::Level(0)),
            (1, Compatibility::Level(1)),
            (2, Compatibility::Level(2)),
            (32768, Compatibility::Newer),
            (65535, Compatibility::Newer),
        ];
        for (wire, compat) in cases {
            let level: Compatibility = (*wire).into();
            assert_eq!(
                level, *compat,
                "wire {wire} should be {compat:?} but got {level}"
            );
            let wire2 = u16::from(*compat);
            if *compat == Compatibility::Newer {
                assert_eq!(wire2, 0, "compat Newer should be wire 0");
            } else {
                assert_eq!(
                    wire2, *wire,
                    "compat {compat:?} failed to convert back (expected {wire})"
                );
            }
        }
    }
}
