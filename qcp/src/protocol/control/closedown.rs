//! ## Closedown Report
// (c) 2024-25 Ross Younger

use crate::protocol::prelude::*;
use int_enum::IntEnum;
use quinn::ConnectionStats;

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
    use super::{ClosedownReport, ClosedownReportV1};
    use crate::protocol::prelude::*;
    use pretty_assertions::assert_eq;
    use quinn::ConnectionStats;

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
}
