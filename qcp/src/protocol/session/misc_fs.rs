//! Miscellaneous filesystem commands
// (c) 2025 Ross Younger

use crate::protocol::session::prelude::*;

#[derive(Serialize, Deserialize, PartialEq, Debug, Default, Clone)]
/// Arguments for the `CreateDirectory` command
/// This was introduced in qcp 0.8 with compatibility level 4.
pub struct CreateDirectoryArgs {
    /// This is the directory name, relative or absolute path.
    pub dir_name: String,

    /// Extended options (not currently used; reserved for future expansion)
    pub options: Vec<TaggedData<CommandParam>>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Default, Clone)]
/// Arguments for the `SetMetadata` command
///
/// This was introduced in qcp 0.8 with compatibility level 4.
pub struct SetMetadataArgs {
    /// This is the path to affect. It may be a relative or absolute path.
    ///
    /// At present only directories are supported.
    pub path: String,

    /// The metadata to apply.
    ///
    /// At present only permissions are supported.
    pub metadata: Vec<TaggedData<MetadataAttr>>,

    /// Extended options (not currently used; reserved for future expansion)
    pub options: Vec<TaggedData<CommandParam>>,
}
