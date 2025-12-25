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

use int_enum::IntEnum;
use serde::{Deserialize, Serialize};
use serde_bare::Uint;
use tracing::debug;

use std::{fmt::Display, fs::Metadata as FsMetadata, time::SystemTime};

use crate::{
    protocol::{
        DataTag, TaggedData, Variant, compat::Feature, control::Compatibility, display_vec_td,
    },
    util::{FsMetadataExt as _, time::SystemTimeExt as _},
};

use super::common::ProtocolMessage;

mod commands;
pub use commands::*;

/// Machine-readable codes advising of the status of an operation.
///
/// See also [`Status::to_string`] which copes correctly with unrecognised status values.
///
/// Note that this enum is serialized without serde_repr, so explicit discriminants cannot meaningfully be used.
/// This also means that the ordering and meaning of existing items cannot be changed without breaking compatibility.
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
    ItIsAFile = 8,
    UnknownError = 9,
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

// ergonomic convenience for tests
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
impl From<anyhow::Error> for Status {
    fn from(e: anyhow::Error) -> Self {
        if let Some(st) = e.downcast_ref::<Status>() {
            return *st;
        }
        if let Some(r) = e.downcast_ref::<Response>() {
            let s = r.status();
            if let Ok(st) = Status::try_from(s) {
                return st;
            }
            // this is test code, it's OK to panic
            panic!("Unknown status code {}", s.0)
        }
        panic!("Expected a Status or a Response");
    }
}

// ergonomic convenience for tests
#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
impl<R: std::fmt::Debug> From<anyhow::Result<R>> for Status {
    fn from(r: anyhow::Result<R>) -> Self {
        Self::from(r.unwrap_err())
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

/////////////////////////////////////////////////////////////////////////////////////////////
// ADDITIONAL OPTIONS AND METADATA

/// Options for commands which take [`Variant`] parameters. (These travel together, as [`TaggedData`].)
///
/// Options are generally only valid on certain commands.
/// Refer to the documentation of the relevant command for details of which enum values are valid where.
///
/// Note that this enum is serialized without serde_repr for forwards compatibility.
/// Therefore, explicit discriminants cannot meaningfully be used.
/// This also means that the ordering and meaning of existing items cannot be changed without breaking compatibility.
///
/// Unknown enum members may be ignored or cause an error.
///
/// Introduced in qcp 0.5 with `VersionCompatibility=V2`.
///
#[derive(PartialEq, Eq, Debug, Clone, Copy, IntEnum, Default, strum_macros::Display)]
#[repr(u64)]
#[non_exhaustive]
pub enum CommandParam {
    /// Invalid option tag. Used for convenience, will not be seen on the wire.
    #[default]
    Invalid,

    /// Preserve file metadata (modes, access and modification times) as far as possible.
    ///
    /// The associated [`Variant`] data is empty (ignored).
    ///
    /// Introduced in qcp 0.5 with `VersionCompatibility=V2`.
    PreserveMetadata,
}
impl DataTag for CommandParam {}

/// Extensible file metadata. These travel with a Variant, as [`TaggedData`].
///
/// Note that this enum is serialized without serde_repr, so explicit discriminants cannot meaningfully be used.
/// This also means that the ordering and meaning of existing items cannot be changed without breaking compatibility.
///
/// Refer to the documentation of the containing struct for details of which enum values are valid where.
#[derive(
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    Clone,
    Copy,
    Debug,
    IntEnum,
    Default,
    strum_macros::Display,
)]
#[repr(u64)]
#[non_exhaustive]
pub enum MetadataAttr {
    /// Invalid metadata tag. Used for convenience, will not be seen on the wire.
    #[default]
    Invalid,

    /// Unix file mode bits to apply _before_ writing the file, as an integer. For example 0o644.
    ///
    /// Variant data is Unsigned.
    ///
    /// If `ModeBits` are not given, the file will be created using the process's umask.
    ///
    /// Introduced in qcp 0.5 with `VersionCompatibility=V2`.
    ModeBits,
    /// Access time to apply to the file, as a Unix timestamp. This may be a 64-bit quantity.
    ///
    /// Variant data is Unsigned.
    ///
    /// If not specified, the file will be created with the current time.
    ///
    /// Introduced in qcp 0.5 with `VersionCompatibility=V2`.
    AccessTime,
    /// Modification time to apply to the file, as a Unix timestamp. This may be a 64-bit quantity.
    ///
    /// Variant data is Unsigned.
    ///
    /// If not specified, the file will be created with the current time.
    ///
    /// Introduced in qcp 0.5 with `VersionCompatibility=V2`.
    ModificationTime,
}
impl DataTag for MetadataAttr {
    fn debug_data(&self, data: &Variant) -> String {
        match self {
            MetadataAttr::ModeBits => match data {
                Variant::Unsigned(mode) => format!("0{:0>3o}", mode.0), // Show octal mode bits
                _ => format!("{data:?}"),
            },
            _ => format!("{data:?}"),
        }
    }
}

impl MetadataAttr {
    /// Convenience constructor for Metadata::AccessTime
    #[must_use]
    pub fn new_atime(t: SystemTime) -> TaggedData<MetadataAttr> {
        Self::AccessTime.with_unsigned(t.to_unix())
    }
    /// Convenience constructor for Metadata::Mode
    #[must_use]
    pub fn new_mode(m: u32) -> TaggedData<MetadataAttr> {
        Self::ModeBits.with_unsigned(m)
    }
    /// Convenience constructor for Metadata::SystemTime
    #[must_use]
    pub fn new_mtime(t: SystemTime) -> TaggedData<MetadataAttr> {
        Self::ModificationTime.with_unsigned(t.to_unix())
    }
}

/////////////////////////////////////////////////////////////////////////////////////////////

/// A command from client to server.
///
/// The server must respond with a Response before anything else can happen on this connection.
#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, strum_macros::Display)]
pub enum Command {
    /// Retrieves a file. This may fail if the file does not exist or the user doesn't have read permission.
    ///
    /// This version does not support file metadata.
    ///
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

    /// Retrieves a file, with additional (extensible) options.
    ///
    /// This command was introduced in qcp 0.5 with `VersionCompatibility=V2`.
    ///
    /// This may fail if the file does not exist or the user doesn't have read permission.
    /// * Client ➡️ Server: `Get` command
    /// * S➡️C: [`Response`], [`FileHeader`], file data, [`FileTrailer`].
    /// * Client closes the stream after transfer.
    /// * If the client needs to abort transfer, it closes the stream.
    /// * If the server needs to abort transfer, it closes the stream.
    Get2(Get2Args),

    /// Sends a file, with additional (extensible) options.
    ///
    /// This command was introduced in qcp 0.5 with `VersionCompatibility=V2`.
    ///
    ///  This may fail for permissions or if the containing directory doesn't exist.
    /// * Client ➡️ Server: `Put` command
    /// * S➡️C: [`Response`] (to the command)
    /// * (if not OK - close stream or send another command)
    /// * C➡️S: [`FileHeader`], file data, [`FileTrailer`]
    /// * S➡️C: [`Response`] (showing transfer status)
    /// * Then close the stream.
    ///
    /// If the server needs to abort the transfer, it may send a Response explaining why, then close the stream.
    Put2(Put2Args),

    /// Ensures that a directory on the remote exists, creating it if necessary.
    ///
    /// This command was introduced in qcp 0.8 with compatibility level 4.
    CreateDirectory(CreateDirectoryArgs),

    /// Updates file or directory metadata on the remote.
    ///
    /// This command was introduced in qcp 0.8 with compatibility level 4.
    SetMetadata(SetMetadataArgs),
}
impl ProtocolMessage for Command {}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Clone)]
/// Arguments for the `GET` command
pub struct GetArgs {
    /// This is a file name, with leading directory components as required
    pub filename: String,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Default, Clone)]
/// Arguments for the `GET2` command.
/// This was introduced in qcp 0.5 with `VersionCompatibility=V2`.
pub struct Get2Args {
    /// This is a file name, with leading directory components as required
    pub filename: String,

    /// Extended options for the GET command
    ///
    /// Supported options: [`CommandParam::PreserveMetadata`]
    pub options: Vec<TaggedData<CommandParam>>,
}
impl From<GetArgs> for Get2Args {
    fn from(v1: GetArgs) -> Self {
        Self {
            filename: v1.filename,
            options: vec![],
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug, Default, Clone)]
/// Arguments for the `PUT` command
pub struct PutArgs {
    /// This is the destination file or directory name, with leading directory components as required.
    /// If it is a directory name, the filename given in the protocol `FileHeader` is appended.
    pub filename: String,
}
#[derive(Serialize, Deserialize, PartialEq, Debug, Default, Clone)]
/// Arguments for the `PUT2` command.
/// This was introduced in qcp 0.5 with `VersionCompatibility=V2`.
pub struct Put2Args {
    /// This is the destination file or directory name, with leading directory components as required.
    /// If it is a directory name, the filename given in the protocol `FileHeader` is appended.
    pub filename: String,

    /// Extended options for the PUT command
    ///
    /// Supported options: [`CommandParam::PreserveMetadata`]
    pub options: Vec<TaggedData<CommandParam>>,
}
impl From<PutArgs> for Put2Args {
    fn from(v1: PutArgs) -> Self {
        Self {
            filename: v1.filename,
            options: vec![],
        }
    }
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

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
/// File Header packet. Metadata sent before file data.
///
/// This is an enum to provide for forward compatibility.
pub enum FileHeader {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    V1(FileHeaderV1),
    /// This version was introduced in qcp 0.5 with `VersionCompatibility=V2`.
    V2(FileHeaderV2),
}
impl ProtocolMessage for FileHeader {
    const WIRE_ENCODING_LIMIT: u32 = 65_536;
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

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
/// Version 2 of [`FileHeader`]
pub struct FileHeaderV2 {
    /// Size of the data that follows.
    /// This is a variable-length word.
    pub size: Uint,
    /// Name of the file. This is a filename only, without any directory component.
    pub filename: String,

    /// Additional metadata to apply to the file.
    /// Valid keys are:
    /// - Mode
    ///   Note that the writing process needs to write to the file, so write permission
    ///   is implicitly added. This can be fixed, if needed, by providing a Mode in the
    ///   [`FileTrailer`].
    ///
    /// N.B. AccessTime and ModificationTime are not valid here. They can only be provided
    /// in the [`FileTrailer`].
    pub metadata: Vec<TaggedData<MetadataAttr>>,
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
    /// Convenience constructor
    #[must_use]
    pub fn new_v2(size: u64, filename: &str, metadata: Vec<TaggedData<MetadataAttr>>) -> Self {
        FileHeader::V2(FileHeaderV2 {
            size: Uint(size),
            filename: filename.to_string(),
            metadata,
        })
    }
    pub(crate) fn for_file(
        compat: Compatibility,
        meta: &FsMetadata,
        protocol_filename: &str,
    ) -> Self {
        if compat.supports(Feature::GET2_PUT2) {
            debug!("Using v2 file header/trailer");
            // Always send mode bits, try to get the permissions as close to correct as possible
            let qcpmeta = meta.to_tagged_data(false);
            debug!("Header metadata: {}", display_vec_td(&qcpmeta));
            FileHeader::new_v2(meta.len(), protocol_filename, qcpmeta)
        } else {
            debug!("Using v1 file header/trailer");
            FileHeader::new_v1(meta.len(), protocol_filename)
        }
    }
}
impl From<FileHeaderV1> for FileHeaderV2 {
    fn from(other: FileHeaderV1) -> Self {
        Self {
            size: other.size,
            filename: other.filename,
            metadata: vec![],
        }
    }
}
impl From<FileHeader> for FileHeaderV2 {
    fn from(value: FileHeader) -> Self {
        match value {
            FileHeader::V2(hdr) => hdr,
            FileHeader::V1(hdr) => hdr.into(),
        }
    }
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
#[repr(u8)]
/// File Trailer packet. Metadata sent after file data.
///
/// This is an enum to provide for forward compatibility.
pub enum FileTrailer {
    /// This version was introduced in qcp 0.3 with `VersionCompatibility=V1`.
    /// It has no contents.
    V1 = 0,
    /// This version was introduced in qcp 0.5 with `VersionCompatibility=V2`.
    V2(FileTrailerV2),
}
impl ProtocolMessage for FileTrailer {
    const WIRE_ENCODING_LIMIT: u32 = 65_536;
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, Default)]
/// Version 2 of [`FileTrailer`]
pub struct FileTrailerV2 {
    /// Additional metadata to apply to the file.
    /// Valid keys are:
    /// - Mode
    /// - AccessTime. If unspecified, the time will be set by the receiving OS.
    /// - ModificationTime. If unspecified, the time will be set by the receiving OS.
    pub metadata: Vec<TaggedData<MetadataAttr>>,
}
impl From<FileTrailer> for FileTrailerV2 {
    fn from(value: FileTrailer) -> Self {
        match value {
            FileTrailer::V2(t) => t,
            FileTrailer::V1 => FileTrailerV2::default(),
        }
    }
}

impl FileTrailer {
    pub(crate) fn for_file(compat: Compatibility, meta: &FsMetadata, preserve: bool) -> Self {
        if compat.supports(Feature::GET2_PUT2) {
            let metadata = if preserve {
                meta.to_tagged_data(true)
            } else {
                Vec::new()
            };
            debug!("Trailer metadata: {}", display_vec_td(&metadata));
            FileTrailer::V2(FileTrailerV2 { metadata })
        } else {
            FileTrailer::V1
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use assertables::assert_contains;
    use pretty_assertions::assert_eq;
    use serde_bare::Uint;

    use crate::protocol::{
        common::ProtocolMessage,
        session::{DataTag, FileHeader, FileTrailer, FileTrailerV2, MetadataAttr, TaggedData},
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
        let FileHeader::V1(h) = h else { panic!() };
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
    fn wire_marshalling_file_header_v2() {
        let head = FileHeader::new_v2(12345, "myfile", vec![MetadataAttr::new_mode(0o644)]);
        let wire = head.to_vec().unwrap();
        let expected = b"\x01\xb9`\x06myfile\x01\x01\x03\xa4\x03".to_vec();
        assert_eq!(wire, expected);
    }

    #[test]
    fn wire_marshalling_file_trailer_v1() {
        let trail = FileTrailer::V1;
        let wire = trail.to_vec().unwrap();
        let expected = b"\x00".to_vec(); // V1 has no contents
        assert_eq!(wire, expected);

        let mut buf = Vec::new();
        trail.to_writer_framed(&mut buf).unwrap();
        eprintln!("{buf:?}");
        assert_eq!(buf.len(), 5);
    }

    #[test]
    fn wire_marshalling_file_trailer_v2() {
        let trail = FileTrailer::V2(FileTrailerV2 {
            metadata: vec![
                MetadataAttr::new_mode(0o644),
                MetadataAttr::AccessTime.with_unsigned(1_700_000_000u64),
                MetadataAttr::ModificationTime.with_unsigned(42u64),
            ],
        });
        let wire = trail.to_vec().unwrap();
        let expected = b"\x01\x03\x01\x03\xa4\x03\x02\x03\x80\xe2\xcf\xaa\x06\x03\x03*".to_vec();
        assert_eq!(wire, expected);

        let mut buf = Vec::new();
        trail.to_writer_framed(&mut buf).unwrap();
        eprintln!("{buf:?}");
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

    #[test]
    fn metadata_debug_render() {
        let t = MetadataAttr::ModeBits.with_unsigned(0o644u32);
        let s = format!("{t:?}");
        eprintln!("case 1: {s}");
        assert!(s.contains("MetadataAttr::ModeBits"));
        assert!(s.contains("data: 0644"));

        // Unknown enum reprs may arise from newer protocol versions
        let t = TaggedData::<MetadataAttr>::new_raw(99999);
        let s = format!("{t:?}");
        eprintln!("case 2: {s}");
        assert!(s.contains("MetadataAttr::UNKNOWN_99999"));
        assert!(s.contains("data: Empty"));
    }
}
