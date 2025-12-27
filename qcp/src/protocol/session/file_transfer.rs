//! Session protocol command structure definitions
// (c) 2025 Ross Younger

use crate::protocol::session::prelude::*;
use crate::util::FsMetadataExt as _;
use std::fs::Metadata as FsMetadata;
use tracing::debug;

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
    use super::{FileHeader, FileHeaderV1, FileTrailer, FileTrailerV2};
    use crate::protocol::session::prelude::*;

    use pretty_assertions::assert_eq;
    use serde_bare::Uint;

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
}
