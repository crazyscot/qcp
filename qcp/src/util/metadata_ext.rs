//! Extension trait for std::fs::Metadata
// (c) 2025 Ross Younger

use crate::protocol::{TaggedData, control::Compatibility, session::MetadataAttr};

/// Extension trait for `std::fs::Metadata`
pub(crate) trait FsMetadataExt: std::marker::Sized {
    /// Extract the Unix mode bits, or a facsimile
    fn mode(&self) -> u32;
    /// Convert filesystem metadata to QCP protocol metadata
    fn to_tagged_data(&self, times: bool) -> Vec<TaggedData<MetadataAttr>>;

    /// Convert filesystem metadata to QCP protocol metadata for a directory
    fn tagged_data_for_dir(&self, compat: Compatibility) -> Vec<TaggedData<MetadataAttr>>;
}

impl FsMetadataExt for std::fs::Metadata {
    #[cfg(unix)]
    fn mode(&self) -> u32 {
        use std::os::unix::fs::PermissionsExt as _;
        self.permissions().mode() & 0o777
    }
    #[cfg(windows)]
    fn mode(&self) -> u32 {
        // Map readonly -> 444, readwrite -> 666.
        // World writable, you say? Well:
        //
        // 1. When copying to a Unix host without `-p`, we apply the user's umask at file creation time.
        // 2. When copying with `-p`, they are explicitly instructing us to make the file world-writable,
        //    as that is how it is on the Windows host. Indeed, this is what you get when you scp a file
        //    from Windows to a Unix host.
        //
        // Windows has no concept of execute permission, so do not send that bit.
        // (TODO, some day: Do something sensible with Windows ACLs.)
        if self.permissions().readonly() {
            0o444
        } else {
            0o666
        }
    }

    fn to_tagged_data(&self, preserve: bool) -> Vec<TaggedData<MetadataAttr>> {
        static_assertions::assert_cfg!(any(unix, windows), "This OS is not currently supported");

        let mode = if cfg!(unix) {
            // It seems that openssh (at least on debian) always gets the mode bits right, even without -p
            self.mode()
        } else {
            // Windows:
            // If the user has explicitly asked us to preserve the mode, then send that (see the comment
            // on `mode()` above).
            // If not, send standard file permission bits.
            //
            // At first glance, 0o666 seems surprisingly insecure, but hold off filing that bug report:
            // When _creating_ a file, qcp client mode applies the user's umask.
            // (See `OpenOptionsExt::apply_qcp_meta.) Therefore, they will typically get mode 0644 or
            // 0664, which is OK. We have to assume that the destination user's umask is set
            // appropriately for their system policy.
            //
            // Then, if the user sets `--preserve`, they are explicitly instructing us to preserve permissions
            // as closely to the source as we can. If they do that, a read-write file on Windows becomes
            // a world-writeable file on Unix (mode 0666).
            // This is the same behaviour as the Windows port of OpenSSH.
            //
            // Windows has no concept of execute permission, so do not send that bit.
            // (TODO, some day: Do something sensible with Windows ACLs.)
            if preserve { self.mode() } else { 0o666 }
        };
        let mut vec = vec![MetadataAttr::new_mode(mode)];
        if preserve {
            if let Ok(t) = self.accessed() {
                vec.push(MetadataAttr::new_atime(t));
            }
            if let Ok(t) = self.modified() {
                vec.push(MetadataAttr::new_mtime(t));
            }
        }
        vec
    }

    fn tagged_data_for_dir(&self, _compat: Compatibility) -> Vec<TaggedData<MetadataAttr>> {
        static_assertions::assert_cfg!(any(unix, windows), "This OS is not currently supported");
        self.to_tagged_data(false)
    }
}
