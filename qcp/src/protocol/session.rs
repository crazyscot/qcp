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
//! * C ➡️ S: [PutArgs] _(within [Command])_
//! * S ➡️ C: [Response] to the command
//! * C ➡️ S: [FileHeader], file data, [FileTrailer].
//! * S ➡️ C: [Response] indicating transfer status
//!
//! After transfer, close the stream.
//!
//! If the server needs to abort the transfer mid-flow, it may send a Response explaining why, then close the stream.
//!
//! [quic]: https://quicwg.github.io/
//! [BARE]: https://www.ietf.org/archive/id/draft-devault-bare-11.html

use serde::{Deserialize, Serialize};
use serde_bare::Uint;
use std::fmt::Display;

use super::common::ProtocolMessage;

/// Machine-readable codes advising of the status of an operation
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
)]
#[repr(u16)]
#[allow(missing_docs)]
pub enum Status {
    Ok = 0,
    FileNotFound = 1,
    IncorrectPermissions = 2,
    DirectoryDoesNotExist = 3,
    IoError = 4,
    DiskFull = 5,
    NotYetImplemented = 6,
    ItIsADirectory = 7,
}

/// A command from client to server.
///
/// The server must respond with a Response before anything else can happen on this connection.
#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, strum::Display)]
#[repr(u16)]
pub enum Command {
    /// Retrieves a file. This may fail if the file does not exist or the user doesn't have read permission.
    /// * Client ➡️ Server: `Get` command
    /// * S➡️C: [`Response`], [`FileHeader`], file data, [`FileTrailer`].
    /// * Client closes the stream after transfer.
    /// * If the client needs to abort transfer, it closes the stream.
    /// * If the server needs to abort transfer, it closes the stream.
    Get(GetArgs) = 1,
    /// Sends a file. This may fail for permissions or if the containing directory doesn't exist.
    /// * Client ➡️ Server: `Put` command
    /// * S➡️C: [`Response`] (to the command)
    /// * (if not OK - close stream or send another command)
    /// * C➡️S: [`FileHeader`], file data, [`FileTrailer`]
    /// * S➡️C: [`Response`] (showing transfer status)
    /// * Then close the stream.
    ///
    /// If the server needs to abort the transfer, it may send a Response explaining why, then close the stream.
    Put(PutArgs) = 2,
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

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
/// Response packet
#[repr(u16)]
pub enum Response {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    V1(ResponseV1) = 1,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone, derive_more::Constructor)]
/// Version 1 of [`Response`]
///
/// This is an enum to provide for forward compatibility.
pub struct ResponseV1 {
    /// Outcome of the operation
    pub status: Status,
    /// A human-readable message giving more information, if any is pertinent
    pub message: Option<String>,
}
impl ProtocolMessage for Response {
    const WIRE_ENCODING_LIMIT: u32 = 65_536;
}

impl Display for ResponseV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.message {
            Some(msg) => write!(f, "{:?} with message {}", self.status, msg),
            None => write!(f, "{:?}", self.status),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Clone)]
/// File Header packet. Metadata sent before file data.
///
/// This is an enum to provide for forward compatibility.
#[repr(u16)]
pub enum FileHeader {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    V1(FileHeaderV1) = 1,
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
mod test {
    use serde_bare::Uint;

    use crate::protocol::{
        common::ProtocolMessage,
        session::{FileHeader, FileTrailer},
    };

    use super::{Command, FileHeaderV1, Response, ResponseV1, Status};

    #[test]
    fn display() {
        let r = ResponseV1 {
            status: Status::Ok,
            message: Some("hi".to_string()),
        };
        assert_eq!(format!("{r}"), "Ok with message hi");
        let r = ResponseV1 {
            status: Status::Ok,
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
            status: Status::ItIsADirectory,
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
}
