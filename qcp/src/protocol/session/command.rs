//! Session protocol command structure definitions
// (c) 2025 Ross Younger

use super::get_put::{Get2Args, GetArgs, Put2Args, PutArgs};
use super::misc_fs::{CreateDirectoryArgs, ListArgs, SetMetadataArgs};
use crate::protocol::prelude::*;
#[allow(unused_imports, reason = "needed for docs")]
use crate::protocol::session::Response;
use crate::util::time::SystemTimeExt as _;
use int_enum::IntEnum;
use std::time::SystemTime;

#[allow(unused_imports, reason = "needed for docs")]
use super::file_transfer::{FileHeader, FileTrailer};

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

    /// Lists the contents of the remote filesystem
    ///
    /// This command was introduced in qcp 0.8 with compatibility level 4.
    ///
    /// * Client ➡️ Server: `Get` command
    /// * S➡️C: [`Response::List`]
    /// * Then close the stream.
    ///
    /// * Either side may close the stream early if it has a problem.
    List(ListArgs),
}
impl ProtocolMessage for Command {}

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

    /// Recurse into the target directory.
    ///
    /// The associated [`Variant`] data is empty (ignored).
    ///
    /// Introduced in qcp 0.8 with compatibility level 4
    Recurse,
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::Command;
    use crate::protocol::session::prelude::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn serialize() {
        let cmd = Command::Get(super::GetArgs {
            filename: "myfile".to_string(),
        });
        let wire = cmd.to_vec().unwrap();
        let deser = Command::from_slice(&wire).unwrap();
        assert_eq!(cmd, deser);
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
