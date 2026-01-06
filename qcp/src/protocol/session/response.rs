//! Session protocol response structure definitions
// (c) 2025 Ross Younger

use crate::{protocol::session::prelude::*, util::FsMetadataExt};
use std::fmt::Display;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone, thiserror::Error)]
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

/// ListResponse was introduced in qcp 0.8 with compatibility level 4.
#[derive(
    Serialize, Deserialize, PartialEq, Debug, Clone, derive_more::Constructor, thiserror::Error,
)]
pub struct ListData {
    /// Response detail
    pub entries: Vec<ListEntry>,
}
impl ProtocolMessage for ListData {
    // Default encoding limit (1M) applies
}

impl Display for ListData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let _ = writeln!(f, "<ListData: [");
        for it in &self.entries {
            let _ = writeln!(f, "{it}");
        }
        writeln!(f, "]>")
    }
}

/// A single file or directory entry returned by a `List` request. See [`Command::List`].
#[derive(
    Serialize, Deserialize, PartialEq, Debug, Clone, derive_more::Constructor, thiserror::Error,
)]
pub struct ListEntry {
    /// Filename (UTF-8)
    pub name: String,
    /// Is this a directory?
    pub directory: bool,
    /// file size in bytes
    pub size: Uint,
    /// Additional metadata for the entry as required.
    ///
    /// Currently supported: [`MetadataAttr::ModeBits`] on directories.
    pub attributes: Vec<TaggedData<MetadataAttr>>,
}

impl Display for ListEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.directory {
            let mode = self.attributes.find_tag(MetadataAttr::ModeBits);
            if let Some(mode) = mode {
                write!(f, "<DIR> {} mode={:o}", self.name, mode.coerce_unsigned())
            } else {
                write!(f, "<DIR> {}", self.name)
            }
        } else {
            write!(f, "      {} {}", self.name, self.size.0)
        }
    }
}

impl From<walkdir::DirEntry> for ListEntry {
    fn from(value: walkdir::DirEntry) -> Self {
        let directory = value.file_type().is_dir();
        let mut attributes = vec![];
        if directory && let Ok(meta) = value.metadata() {
            attributes.push(MetadataAttr::new_mode(meta.mode()));
        }
        Self {
            name: value.path().to_string_lossy().to_string(), // relative to root!
            directory,
            size: Uint(value.metadata().map_or(0, |m| m.len())),
            attributes,
        }
    }
}

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
    EncodingFailed = 10,
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::{Response, ResponseV1, Status};
    use crate::protocol::session::{ListData, ListEntry, prelude::*};
    use assertables::assert_contains;
    use pretty_assertions::assert_eq;

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
    fn serialize() {
        let resp = Response::V1(ResponseV1 {
            status: Status::ItIsADirectory.into(),
            message: Some("nope".to_string()),
        });
        let wire = resp.to_vec().unwrap();
        let deser = Response::from_slice(&wire).unwrap();
        assert_eq!(resp, deser);
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
    fn list_contents_display() {
        let lc = ListData {
            entries: vec![
                ListEntry {
                    name: "aaa".to_string(),
                    directory: false,
                    size: Uint(42),
                    attributes: vec![],
                },
                ListEntry {
                    name: "bbb".to_string(),
                    directory: true,
                    size: Uint(0),
                    attributes: vec![],
                },
            ],
        };
        let str = lc.to_string();
        eprintln!("{str}");
        assert_contains!(str, "aaa");
        assert_contains!(str, "bbb");
    }
}
