//! QCP session protocol definitions and helper types
// (c) 2024 Ross Younger
//!
//! The session protocol operates over a QUIC bidirectional stream.
//!
//! The protocol consists of [Command] and [Response] packets and helper structs.
//!
//! * Client ➡️ Server: (initiates QUIC stream)
//! * C ➡️ S : [Command] packet. This is an enum containing arguments needed by the selected command.
//! * S ➡️ C : [Response] packet
//! * Then they do whatever is appropriate for the command.
//!
//! The following commands are defined:
//! ### Get
//!
//! Retrieves a file from the remote.
//! * C ➡️ S: [GetArgs] _(within [Command])_
//! * S ➡️ C: [Response] . If the status within was not OK, the command does not proceed.
//! * S ➡️ C: [FileHeader], file data, [FileTrailer].
//!
//! After transfer, close the stream.
//!
//! Either side may close the stream mid-flow if it needs to abort the transfer.
//!
//! ### Put
//!
//! Sends a file to the remote.
//! * C ➡️ S: [PutArgs] _(within [Command])_, [FileHeader] _(see note!)_
//! * S ➡️ C: [Response] to the command.
//!   The server has already opened the destination file for writing, so has applied permission checks.
//!   If the status is not OK, the command does not proceed.
//! * C ➡️ S: file data, [FileTrailer].
//! * S ➡️ C: [Response] indicating transfer status
//!
//! _N.B. In versions 0.3.0 through to 0.3.3, the server's [Response] was sent between [PutArgs] and [FileHeader].
//!  This is a minor protocol refinement that improves reliability and testability without affecting compatibility._
//!
//! After transfer, close the stream.
//!
//! If the server needs to abort the transfer mid-flow, it may send a Response explaining why, then close the stream.
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

use serde::{Deserialize, Serialize};
use serde_bare::Uint;
use std::fmt::Display;

use super::common::ProtocolMessage;

/// Machine-readable codes advising of the status of an operation.
///
/// See also [`Status::to_string`] which copes correctly with unrecognised status values.
#[derive(
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Debug,
    Clone,
    Copy,
    thiserror::Error,
    strum_macros::Display,
    strum_macros::FromRepr,
)]
#[allow(missing_docs)]
#[non_exhaustive]
pub enum Status {
    // Note that this enum is serialized without serde_repr, so explicit discriminants are not used on the wire.
    // This also means that the ordering and meaning of existing items cannot be changed without breaking compatibility.
    //
    // CAUTION: CompatibilityLevel 1 panics when unmarshalling statuses above 7
    Ok = 0,
    FileNotFound = 1,
    IncorrectPermissions = 2,
    DirectoryDoesNotExist = 3,
    IoError = 4,
    DiskFull = 5,
    NotYetImplemented = 6,
    ItIsADirectory = 7,
    // CAUTION: CompatibilityLevel 1 panics when unmarshalling statuses above 7
}

impl From<Status> for Uint {
    fn from(value: Status) -> Self {
        Self(value as u64)
    }
}

impl TryFrom<Uint> for Status {
    type Error = anyhow::Error;

    fn try_from(value: Uint) -> Result<Self, Self::Error> {
        #[allow(clippy::cast_possible_truncation)]
        Status::from_repr(value.0 as usize).ok_or_else(|| anyhow::anyhow!("unknown status code"))
    }
}

impl Status {
    /// String conversion function for a Uint that holds a Status value
    #[must_use]
    pub fn to_string(value: Uint) -> String {
        Status::try_from(value).map_or_else(
            |_| format!("Unknown status code {}", value.0),
            |st| st.to_string(),
        )
    }
}

impl PartialEq<Uint> for Status {
    fn eq(&self, other: &Uint) -> bool {
        *self as u64 == other.0
    }
}

impl PartialEq<Status> for Uint {
    fn eq(&self, other: &Status) -> bool {
        self.0 == *other as u64
    }
}

/// A command from client to server.
///
/// The server must respond with a Response before anything else can happen on this connection.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, strum_macros::Display)]
pub enum Command {
    /// Retrieves a file. This may fail if the file does not exist or the user doesn't have read permission.
    /// * Client ➡️ Server: `Get` command
    /// * S➡️C: [`Response`], [`FileHeader`], file data, [`FileTrailer`].
    /// * Client closes the stream after transfer.
    /// * If the client needs to abort transfer, it closes the stream.
    /// * If the server needs to abort transfer, it closes the stream.
    Get(GetArgs),
    /// Sends a file. This may fail for permissions or if the containing directory doesn't exist.
    /// * Client ➡️ Server: `Put` command
    /// * S➡️C: [`Response`] (to the command)
    /// * (if not OK - close stream or send another command)
    /// * C➡️S: [`FileHeader`], file data, [`FileTrailer`]
    /// * S➡️C: [`Response`] (showing transfer status)
    /// * Then close the stream.
    ///
    /// If the server needs to abort the transfer, it may send a Response explaining why, then close the stream.
    Put(PutArgs),
}
impl ProtocolMessage for Command {}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Clone)]
/// Arguments for the `GET` command
pub struct GetArgs {
    /// This is a file name only, without any directory components
    pub filename: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Clone)]
/// Arguments for the `PUT` command
pub struct PutArgs {
    /// This is a file name only, without any directory components
    pub filename: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, thiserror::Error)]
#[error(transparent)]
/// Response packet
pub enum Response {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    V1(ResponseV1),
}

impl Response {
    pub(crate) fn status(&self) -> Uint {
        match self {
            Response::V1(r) => r.status,
        }
    }

    /// Wraps this struct up as a Result
    pub(crate) fn into_result(self) -> anyhow::Result<Self> {
        let st = self.status();
        if st == Status::Ok {
            return Ok(self);
        }
        Err(anyhow::Error::new(self))
    }
}

#[derive(
    Serialize, Deserialize, PartialEq, Eq, Debug, Clone, derive_more::Constructor, thiserror::Error,
)]
/// Version 1 of [`Response`]
///
/// This is an enum to provide for forward compatibility.
pub struct ResponseV1 {
    /// Outcome of the operation.
    /// This is a [`Status`] code, but as of CompatibilityLevel 2 it may be outside of the set of values we know.
    pub status: Uint,
    /// A human-readable message giving more information, if any is pertinent
    pub message: Option<String>,
}
impl ProtocolMessage for Response {
    const WIRE_ENCODING_LIMIT: u32 = 65_536;
}

impl Display for ResponseV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = Status::to_string(self.status);
        match &self.message {
            Some(msg) => write!(f, "{str} with message {msg}"),
            None => write!(f, "{str}"),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
/// File Header packet. Metadata sent before file data.
///
/// This is an enum to provide for forward compatibility.
pub enum FileHeader {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    V1(FileHeaderV1),
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
/// Version 1 of [`FileHeader`]
pub struct FileHeaderV1 {
    /// Size of the data that follows.
    /// This is a variable-length word.
    pub size: Uint,
    /// Name of the file. This is a filename only, without any directory component.
    pub filename: String,
}
impl ProtocolMessage for FileHeader {
    const WIRE_ENCODING_LIMIT: u32 = 65_536;
}

impl FileHeader {
    #[must_use]
    /// Convenience constructor
    pub fn new_v1(size: u64, filename: &str) -> Self {
        FileHeader::V1(FileHeaderV1 {
            size: Uint(size),
            filename: filename.to_string(),
        })
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, Copy)]
/// File Trailer packet. Metadata sent after file data.
///
/// This is an enum to provide for forward compatibility.
pub enum FileTrailer {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    /// It has no contents. Future versions may introduce some sort of checksum.
    V1 = 1,
}
impl ProtocolMessage for FileTrailer {
    const WIRE_ENCODING_LIMIT: u32 = 65_536;
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use assertables::assert_contains;
    use pretty_assertions::assert_eq;
    use serde_bare::Uint;

    use crate::protocol::{
        common::ProtocolMessage,
        session::{FileHeader, FileTrailer},
    };

    use super::{Command, FileHeaderV1, Response, ResponseV1, Status};

    #[test]
    fn display() {
        let r = ResponseV1 {
            status: Status::Ok.into(),
            message: Some("hi".to_string()),
        };
        assert_eq!(format!("{r}"), "Ok with message hi");
        let r = ResponseV1 {
            status: Status::Ok.into(),
            message: None,
        };
        assert_eq!(format!("{r}"), "Ok");
    }
    #[test]
    fn ctor() {
        let h = FileHeader::new_v1(42, "myfile");
        let FileHeader::V1(h) = h;
        assert_eq!(h.size.0, 42);
        assert_eq!(h.filename, "myfile");
    }

    #[test]
    fn serialize_command() {
        let cmd = Command::Get(super::GetArgs {
            filename: "myfile".to_string(),
        });
        let wire = cmd.to_vec().unwrap();
        let deser = Command::from_slice(&wire).unwrap();
        assert_eq!(cmd, deser);
    }

    #[test]
    fn serialize_response() {
        let resp = Response::V1(ResponseV1 {
            status: Status::ItIsADirectory.into(),
            message: Some("nope".to_string()),
        });
        let wire = resp.to_vec().unwrap();
        let deser = Response::from_slice(&wire).unwrap();
        assert_eq!(resp, deser);
    }

    #[test]
    fn serialize_file_header_trailer() {
        let head = FileHeader::V1(FileHeaderV1 {
            size: Uint(12345),
            filename: "myfile".to_string(),
        });
        let wire = head.to_vec().unwrap();
        let deser = FileHeader::from_slice(&wire).unwrap();
        assert_eq!(head, deser);

        let trail = FileTrailer::V1;
        let wire = trail.to_vec().unwrap();
        let deser = FileTrailer::from_slice(&wire).unwrap();
        assert_eq!(trail, deser);
    }

    #[test]
    fn wire_marshalling_command_get() {
        let cmd = Command::Get(super::GetArgs {
            filename: "myfile".to_string(),
        });
        let wire = cmd.to_vec().unwrap();
        let expected = b"\x00\x06myfile".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_command_put() {
        let cmd = Command::Put(super::PutArgs {
            filename: "myfile2".to_string(),
        });
        let wire = cmd.to_vec().unwrap();
        let expected = b"\x01\x07myfile2".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_response_v1() {
        let resp = Response::V1(ResponseV1 {
            status: Status::IoError.into(),
            message: Some("hi".to_string()),
        });
        let wire = resp.to_vec().unwrap();
        let expected = b"\x00\x04\x01\x02hi".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_file_header_v1() {
        let head = FileHeader::V1(FileHeaderV1 {
            size: Uint(12345),
            filename: "myfile".to_string(),
        });
        let wire = head.to_vec().unwrap();
        let expected = b"\x00\xb9`\x06myfile".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_file_trailer_v1() {
        let trail = FileTrailer::V1;
        let wire = trail.to_vec().unwrap();
        let expected = b"\x00".to_vec(); // V1 has no contents
        assert_eq!(wire, expected);
    }

    #[test]
    fn unknown_status_doesnt_crash() {
        // hand-created: an outrageously large Status value (2,097,151)
        let wire = &[0u8, 255, 255, 127, 0];

        let deser = Response::from_slice(wire).unwrap();
        eprintln!("{deser:?}");
    }

    #[test]
    fn status_equality() {
        let st = Status::DiskFull;
        let u = Uint::from(st);
        assert_eq!(st, u);
        assert_eq!(u, st);
    }

    #[test]
    fn unknown_status_to_string() {
        let u = Uint(2u64.pow(63));
        assert_contains!(Status::to_string(u), "Unknown status code");
    }
}
