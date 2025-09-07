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
use figment::{
    Profile, Provider,
    value::{Dict, Map},
};
use int_enum::IntEnum;
use num_traits::AsPrimitive;
use quinn::ConnectionStats;
use serde::{Deserialize, Serialize};
use serde_bare::Uint;
use serde_repr::{Deserialize_repr, Serialize_repr};

use super::common::ProtocolMessage;
use crate::{
    Configuration,
    config::Configuration_Optional,
    protocol::{DataTag, FindTag as _, TaggedData, Variant, compat::Feature, display_vec_td},
    transport::ThroughputMode,
    util::{PortRange as CliPortRange, serialization::SerializeAsString},
};

/// Server banner message, sent on stdout and checked by the client
pub const BANNER: &str = "qcp-server-2\n";

/// The banner for the initial protocol version (pre-v0.3) that we don't support any more.
/// Note that it is the same size as the current [`BANNER`].
pub const OLD_BANNER: &str = "qcp-server-1\n";

/// The protocol compatibility version implemented by this crate
pub(crate) const OUR_COMPATIBILITY_NUMERIC: u16 = 3;
/// The protocol compatibility version implemented by this crate
pub const OUR_COMPATIBILITY_LEVEL: Compatibility = Compatibility::Level(OUR_COMPATIBILITY_NUMERIC);

////////////////////////////////////////////////////////////////////////////////////////
// Display helpers

use engineering_repr::{EngineeringQuantity as EQ, EngineeringRepr};

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
/// See [`Feature`] for a mapping from compatibility levels to specific features.
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
/// To serialize it as a string, see [`SerializeAsString`].
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
impl ProtocolMessage for ClientGreeting {
    const WIRE_ENCODING_LIMIT: u32 = 4_096;
}

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
impl ProtocolMessage for ServerGreeting {
    const WIRE_ENCODING_LIMIT: u32 = 4_096;
}

////////////////////////////////////////////////////////////////////////////////////////
// CLIENT MESSAGE

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
            credentials: CredentialsType::X509.with_variant(v1.cert.into()),
            connection_type: v1.connection_type,
            attributes,
            extension: 0,
        }
    }
}

#[cfg(test)]
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug, Default)]
/// The control parameters send from client to server.
enum OriginalClientMessage {
    #[default]
    #[serde(skip_serializing)]
    ToFollow,
    V1(OriginalClientMessageV1),
}
#[cfg(test)]
impl ProtocolMessage for OriginalClientMessage {}

#[cfg(test)]
#[derive(Clone, Serialize, Deserialize, PartialEq, Default, derive_more::Debug)]
struct OriginalClientMessageV1 {
    cert: Vec<u8>,
    connection_type: ConnectionType,
    port: Option<PortRange_OnWire>,
    show_config: bool,
    bandwidth_to_server: Option<Uint>,
    bandwidth_to_client: Option<Uint>,
    rtt: Option<u16>,
    congestion: Option<CongestionController>,
    initial_congestion_window: Option<Uint>,
    timeout: Option<u16>,
    extension: u8,
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
    fn new(
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

////////////////////////////////////////////////////////////////////////////////////////
// SERVER MESSAGE

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
                congestion: *config.congestion,
                initial_congestion_window: Uint(config.initial_congestion_window.into()),
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
    // OPTIONAL: name?
    /// Extension field, reserved for future expansion; for now, must be set to 0
    pub extension: u8,
}

impl From<ServerMessageV1> for ServerMessageV2 {
    fn from(v1: ServerMessageV1) -> Self {
        let mut attributes = Vec::new();
        if !v1.warning.is_empty() {
            attributes
                .push(ServerMessage2Attributes::WarningMessage.with_variant(v1.warning.into()));
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
            credentials: CredentialsType::X509.with_variant(v1.cert.into()),
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
        if *config.congestion != CongestionController::default() {
            self.attributes.push(
                ServerMessage2Attributes::CongestionController
                    .with_unsigned(*config.congestion as u64),
            );
        }
        let window = u64::from(config.initial_congestion_window);
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

////////////////////////////////////////////////////////////////////////////////////////
// CLOSEDOWN REPORT

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
/// The statistics sent by the server when the job is done
pub enum ClosedownReport {
    /// Special value that should never be seen on the wire
    #[serde(skip_serializing)]
    Unknown,
    /// This version was introduced in qcp 0.3 with `VersionCompatibility` level 1.
    /// On the wire enum discriminant: 1
    V1(ClosedownReportV1),
}
impl ProtocolMessage for ClosedownReport {}

/// Version 1 of the closedown report.
/// This version was introduced in qcp 0.3 with `VersionCompatibility` level 1.
#[derive(Serialize, Deserialize, PartialEq, Default, Clone, derive_more::Debug)]
pub struct ClosedownReportV1 {
    /// Final congestion window
    #[debug("{}", cwnd.0)]
    pub cwnd: Uint,
    /// Number of packets sent
    #[debug("{}", sent_packets.0)]
    pub sent_packets: Uint,
    /// Number of packets lost
    #[debug("{}", lost_packets.0)]
    pub lost_packets: Uint,
    /// Number of bytes lost
    #[debug("{}", lost_bytes.0)]
    pub lost_bytes: Uint,
    /// Number of congestion events detected
    #[debug("{}", congestion_events.0)]
    pub congestion_events: Uint,
    /// Number of black holes detected
    #[debug("{}", black_holes.0)]
    pub black_holes: Uint,
    /// Number of bytes sent
    #[debug("{}", sent_bytes.0)]
    pub sent_bytes: Uint,

    /// Optional extended data
    ///
    /// If it is mandatory for the client to action a given attribute, it MUST NOT be sent in this field.
    /// Instead, use a later version of the `ClosedownReport`.
    ///
    /// This field was added in qcp 0.5 with `VersionCompatibility` level 2.
    /// Prior to Compatibility::Level(2) this was a reserved u8, which was required to be set to 0.
    /// If length 0, it looks the same on the wire.
    /// If length >0, earlier versions ignore the attributes.
    pub extension: Vec<TaggedData<ClosedownReportExtension>>,
}

impl From<&ConnectionStats> for ClosedownReportV1 {
    fn from(stats: &ConnectionStats) -> Self {
        let ps = &stats.path;
        // look, nobody will overrun u64 micros except on interstellar connections (but if so, they won't be using qcp)
        let rtt: u64 = ps.rtt.as_micros().try_into().unwrap_or(u64::MAX);
        let mut extension = vec![];
        if rtt != 0 {
            extension.push(ClosedownReportExtension::Rtt.with_unsigned(rtt));
        }
        if ps.current_mtu != 0 {
            extension.push(ClosedownReportExtension::Pmtu.with_unsigned(ps.current_mtu));
        }

        Self {
            cwnd: Uint(ps.cwnd),
            sent_packets: Uint(ps.sent_packets),
            sent_bytes: Uint(stats.udp_tx.bytes),
            lost_packets: Uint(ps.lost_packets),
            lost_bytes: Uint(ps.lost_bytes),
            congestion_events: Uint(ps.congestion_events),
            black_holes: Uint(ps.black_holes_detected),
            extension,
        }
    }
}

/// Extension attributes for the closedown report
///
/// This enum was introduced in qcp 0.5 with `VersionCompatibility` level 2.
#[derive(strum_macros::Display, Clone, Copy, Debug, IntEnum, PartialEq)]
#[non_exhaustive]
#[repr(u64)]
pub enum ClosedownReportExtension {
    /// Indicates an invalid attribute.
    Invalid = 0,
    /// The Path MTU for the connection, as measured by the server, in bytes
    Pmtu = 1,
    /// The Round Trip Time for the connection, as measured by the server, in microseconds
    Rtt = 2,
}
impl DataTag for ClosedownReportExtension {}

// //////////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use std::{
        io::Cursor,
        net::{IpAddr, Ipv4Addr, Ipv6Addr},
    };

    use assertables::{assert_contains, assert_matches};
    use pretty_assertions::{assert_eq, assert_str_eq};
    use quinn::ConnectionStats;
    use serde::{Deserialize, Serialize};
    use serde_bare::Uint;

    use crate::{
        config::{Configuration, Configuration_Optional, Manager},
        protocol::{
            DataTag, FindTag, TaggedData,
            common::ProtocolMessage,
            control::{
                ClientMessage2Attributes, ClientMessageAttributes, ClientMessageV1,
                ClientMessageV2, ClosedownReport, Compatibility, CongestionController,
                ConnectionType, CredentialsType, Direction, OriginalClientMessage,
                OriginalClientMessageV1, ServerGreeting, ServerMessage2Attributes, ServerMessageV1,
                ServerMessageV2,
            },
            display_vec_td,
        },
        util::{PortRange as CliPortRange, serialization::SerializeAsString},
    };

    use super::{
        ClientGreeting, ClientMessage, ClosedownReportV1, PortRange_OnWire, ServerFailure,
        ServerMessage,
    };

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
            compatibility: 1,
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
            compatibility: 1,
            extension: 0,
        };
        let wire = msg.to_vec().unwrap();
        let deser = ServerGreeting::from_slice(&wire).unwrap();
        assert_eq!(msg, deser);
    }

    fn dummy_cert() -> Vec<u8> {
        vec![0, 1, 2]
    }

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
            let cert = CredentialsType::X509.with_variant(dummy_cert().into());
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
        let cert = CredentialsType::X509.with_variant(dummy_cert().into());
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
    fn serialize_provide_server_message() {
        use engineering_repr::EngineeringQuantity as EQ;

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
            rx: EQ::<u64>::from(v1.bandwidth_to_client.0),
            tx: EQ::<u64>::from(v1.bandwidth_to_server.0),
            rtt: v1.rtt,
            congestion: v1.congestion.into(),
            initial_congestion_window: v1.initial_congestion_window.0.into(),
            timeout: v1.timeout,
            ..Configuration::system_default().clone()
        };
        assert_eq!(cfg, expected);
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
            extension: vec![],
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
    fn compat_level_from_wire() {
        let cases = &[
            (0u16, Compatibility::Level(0)),
            (1, Compatibility::Level(1)),
            (2, Compatibility::Level(2)),
            (32768, Compatibility::Newer),
            (65535, Compatibility::Newer),
        ];
        for (wire, expected) in cases {
            let level: Compatibility = (*wire).into();
            assert_eq!(
                level, *expected,
                "wire {wire} should be {expected:?} but got {level}"
            );
        }
    }

    #[test]
    fn wire_marshalling_client_greeting() {
        // This message is critical to the entire protocol. It cannot change without breaking compatibility.
        let msg = ClientGreeting {
            compatibility: 1,
            debug: true,
            extension: 3,
        };
        let wire = msg.to_vec().unwrap();
        let expected = b"\x01\x00\x01\x03".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_server_greeting() {
        // This message is critical to the entire protocol. It cannot change without breaking compatibility.
        let msg = ServerGreeting {
            compatibility: 1,
            extension: 4,
        };
        let wire = msg.to_vec().unwrap();
        let expected = b"\x01\x00\x04".to_vec();
        assert_eq!(wire, expected);
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
    fn wire_marshalling_closedown_report_v1() {
        let msg = ClosedownReport::V1(ClosedownReportV1 {
            cwnd: Uint(42),
            sent_packets: Uint(65),
            lost_packets: Uint(66),
            lost_bytes: Uint(456_798),
            congestion_events: Uint(44),
            black_holes: Uint(49),
            sent_bytes: Uint(987_654),
            extension: vec![],
        });
        let wire = msg.to_vec().unwrap();
        let expected = b"\x01*AB\xde\xf0\x1b,1\x86\xa4<\x00".to_vec();
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
            congestion: Some(CongestionController::Bbr.into()),
            initial_congestion_window: Some(42u64.into()),
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
                ServerMessage2Attributes::WarningMessage.with_variant("hi".into()),
                ServerMessage2Attributes::Invalid.into(),
            ],
            ..Default::default()
        };
        let mut mgr = Manager::without_files(None);
        mgr.apply_system_default();
        mgr.merge_provider(msg);
        let cfg = mgr.get::<Configuration>().unwrap();
        assert_eq!(cfg.rx, 54321u64.into());
        assert_eq!(cfg.tx, 12345u64.into());
        assert_eq!(cfg.rtt, 42);
        assert_eq!(cfg.congestion, CongestionController::Bbr.into());
        assert_eq!(cfg.initial_congestion_window, 5544u32.into());
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
