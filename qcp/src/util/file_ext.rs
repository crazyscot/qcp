//! Extension traits for tokio::fs::File and related structures
// (c) 2025 Ross Younger

use crate::protocol::TaggedData;
use crate::protocol::session::{FileHeaderV2, MetadataAttr};
use crate::util::time::SystemTimeExt as _;

use std::time::SystemTime;
use std::{
    fs::FileTimes,
    path::{Path, PathBuf},
};

use async_trait::async_trait;
use cfg_if::cfg_if;
use tokio::fs::File as TokioFile;

/// Extension trait for tokio::fs::OpenOptions
trait OpenOptionsExt {
    /// Extract and apply file permissions from the passed-in metadata, for use when creating a file.
    ///
    /// * On Unix, we apply the user's umask in the expected way.
    /// * On Windows, this is a no-op on Windows; we *always* open the file with default permissions.
    ///
    /// When --preserve mode is engaged, the other side sends the desired mode bits in the `FileTrailer`.
    fn apply_qcp_meta(&mut self, qcp_meta: &[TaggedData<MetadataAttr>]);
}

impl OpenOptionsExt for tokio::fs::OpenOptions {
    #[cfg(unix)]
    fn apply_qcp_meta(&mut self, qcp_meta: &[TaggedData<MetadataAttr>]) {
        use crate::protocol::session::MetadataAttr;

        for md in qcp_meta {
            if md.tag().unwrap_or_default() == MetadataAttr::ModeBits {
                if let Some(m) = md.data.as_unsigned_ref() {
                    // SAFETY: The file_mode crate calls `libc::umask()` which is potentially unsafe.
                    // 1. It is a C binding, which is automatically considered unsafe.
                    //    This is not a concern as this is a well-known libc function with well-known behaviour.
                    // 2. The underlying syscall may modify program global state.
                    //    However this library function does so in a safe way: it calls
                    //    `umask(0)`, which does not modify anything. Therefore it is safe.
                    //    (Curveball: `libc::set_umask()` is _not_ marked as unsafe, despite
                    //     modifying program global state! But we don't call that.)
                    let bits = (m & 0o777) as u32 & !file_mode::umask();
                    tracing::debug!(
                        "inbound file mode {m:03o}; umask {:03o} => creat mode {bits:03o}",
                        file_mode::umask()
                    );
                    let _ = self.mode(bits);
                }
            }
        }
    }

    #[cfg(windows)]
    fn apply_qcp_meta(&mut self, _qcp_meta: &[TaggedData<MetadataAttr>]) {
        // No-op at file creation time
    }
}

#[async_trait]
/// Extension trait for `tokio::fs::File`
pub(crate) trait FileExt {
    /// Opens a local file for reading, returning a filehandle and metadata.
    async fn open_with_meta<P: AsRef<Path> + Send>(
        path: P,
    ) -> anyhow::Result<(TokioFile, std::fs::Metadata), tokio::io::Error>;

    /// Opens a local file for writing, from an incoming `FileHeader`
    async fn create_or_truncate<P: AsRef<Path> + Send>(
        path: P,
        header: &FileHeaderV2,
    ) -> anyhow::Result<TokioFile>;

    /// Update file metadata to match the passed-in set.
    ///
    /// NOTE: This function necessarily consumes and re-wraps the given File.
    /// This works around a tokio limitation; see commentary within.
    async fn update_metadata(
        self,
        metadata: &[TaggedData<MetadataAttr>],
    ) -> anyhow::Result<TokioFile>;
}

#[async_trait]
impl FileExt for TokioFile {
    #[allow(
        renamed_and_removed_lints, // for elided_named_lifetimes
        elided_named_lifetimes, // renamed to mismatched_lifetime_syntaxes in rust 1.89
        clippy::type_complexity,
        clippy::type_repetition_in_bounds
    )]
    async fn open_with_meta<P: AsRef<Path> + Send>(
        path: P,
    ) -> anyhow::Result<(TokioFile, std::fs::Metadata), tokio::io::Error> {
        let fh: TokioFile = TokioFile::open(path).await?;
        let meta = fh.metadata().await?;
        // Disallow reading from non-regular files (sockets, device nodes)
        if !meta.is_file() && !meta.is_dir() {
            return Err(tokio::io::Error::new(
                tokio::io::ErrorKind::Unsupported,
                "Source is not a regular file",
            ));
        }
        Ok((fh, meta))
    }

    async fn create_or_truncate<P: AsRef<Path> + Send>(
        path: P,
        header: &FileHeaderV2,
    ) -> anyhow::Result<TokioFile> {
        use OpenOptionsExt as _;

        let mut dest_path = PathBuf::from(path.as_ref());
        let dest_meta = tokio::fs::metadata(&dest_path).await;
        if let Ok(meta) = dest_meta {
            // if it's a file, proceed (overwriting)
            if meta.is_dir() {
                dest_path.push(header.filename.clone());
            } else if !meta.is_file() {
                // Disallow writing to pre-existing non-regular files (sockets, device nodes)
                anyhow::bail!("Destination path exists but is not a regular file");
            }
        } // error ignored; file doesn't exist is perfectly OK with us :-)
        let mut options = tokio::fs::OpenOptions::new();
        let _ = options.create(true).truncate(true);
        options.apply_qcp_meta(&header.metadata);
        Ok(options.write(true).open(&dest_path).await?)
    }

    async fn update_metadata(
        self,
        metadata: &[TaggedData<MetadataAttr>],
    ) -> anyhow::Result<TokioFile> {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt as _;

        let meta = self.metadata().await?;
        let mut times = FileTimes::default();
        let mut changed = false;
        for md in metadata {
            let tag = match md.tag() {
                None | Some(MetadataAttr::Invalid) => continue,
                Some(v) => v,
            };
            match tag {
                MetadataAttr::Invalid => (),
                MetadataAttr::ModeBits => {
                    let mut perms = meta.permissions();
                    if let Some(mode) = md.data.as_unsigned_ref() {
                        let mode = (mode & 0o777) as u32;
                        static_assertions::assert_cfg!(
                            any(unix, windows),
                            "This OS is not currently supported"
                        );
                        cfg_if! {
                            if #[cfg(unix)] {
                                perms.set_mode(mode);
                            }
                            else if #[cfg(windows)] {
                                // Map _any_ writable bits into writeability.
                                let write = (mode & 0o222) != 0;
                                perms.set_readonly(!write);
                            }
                        }
                        self.set_permissions(perms).await?;
                    }
                }
                MetadataAttr::AccessTime => {
                    if let Some(t) = md.data.as_unsigned_ref() {
                        changed = true;
                        times = times.set_accessed(SystemTime::from_unix(*t));
                    }
                }
                MetadataAttr::ModificationTime => {
                    if let Some(t) = md.data.as_unsigned_ref() {
                        changed = true;
                        times = times.set_modified(SystemTime::from_unix(*t));
                    }
                }
            }
        }

        if changed {
            /* Unfortunately, tokio doesn't currently provide an analogue to `std::fs::set_times()`.
             * https://github.com/tokio-rs/tokio/issues/6368 refers.
             * Work around it in the way suggested by that ticket.
             * I don't much like the idea of a spawn_blocking call, would love something lighter
             * weight; but making a blocking fs call here risks breaking concurrency (e.g.
             * writing to an NFS filesystem that might block indeterminately). */
            let std_file = self.into_std().await;
            let file = tokio::task::spawn_blocking(move || {
                std_file.set_times(times)?;
                Ok::<TokioFile, std::io::Error>(TokioFile::from_std(std_file))
            })
            .await??;
            return Ok(file);
        }
        Ok(self)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    #![allow(dead_code)] // windows
    use std::path::PathBuf;

    use crate::protocol::session::FileHeaderV2;
    use crate::util::FileExt as _;

    use littertray::LitterTray;
    use tokio::fs::File as TokioFile;

    const NEW_LEN: u64 = 42;
    const FILE: &str = "file1";
    const FILE_LINK: &str = "file2";
    const DIR: &str = "dir1";
    const DIR_LINK: &str = "dir2";
    const BROKEN_LINK: &str = "file99";
    const BROKEN_LINK_DEST: &str = "file98";
    const FILENAME_IN_HEADER: &str = "xyzy";

    fn setup(tray: &mut LitterTray) -> anyhow::Result<FileHeaderV2> {
        let _ = tray.create_text(FILE, "12345")?;
        let _ = tray.make_dir(DIR)?;
        #[cfg(unix)]
        {
            let _ = tray.make_symlink(FILE, FILE_LINK)?;
            let _ = tray.make_symlink(DIR, DIR_LINK)?;
            let _ = tray.make_symlink(BROKEN_LINK_DEST, BROKEN_LINK);
        }

        Ok(FileHeaderV2 {
            size: serde_bare::Uint(NEW_LEN),
            filename: FILENAME_IN_HEADER.to_string(),
            metadata: vec![],
        })
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dest_is_symlink_to_file() {
        LitterTray::try_with_async(async |tray| {
            let header = setup(tray).unwrap();
            let _f = TokioFile::create_or_truncate(FILE_LINK, &header).await?;
            // Expected outcome: file1 is now truncated, file2 is still a symlink to it.
            let meta1 = tokio::fs::metadata("file1").await.unwrap();
            assert!(meta1.is_file() && meta1.len() == 0);
            let meta2 = tokio::fs::symlink_metadata("file2").await.unwrap();
            assert!(meta2.is_symlink());
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dest_is_dir() {
        LitterTray::try_with_async(async |tray| {
            let header = setup(tray).unwrap();
            let _f = TokioFile::create_or_truncate(DIR, &header).await?;
            // Expected outcome: dir1/xyzy exists
            let mut pb = PathBuf::new();
            pb.push(DIR);
            pb.push(FILENAME_IN_HEADER);
            let meta1 = tokio::fs::metadata(&pb).await.unwrap();
            assert!(meta1.is_file() && meta1.len() == 0);
            Ok(())
        })
        .await
        .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dest_is_symlink_to_dir() {
        LitterTray::try_with_async(async |tray| {
            let header = setup(tray).unwrap();
            let _f = TokioFile::create_or_truncate(DIR_LINK, &header).await?;
            // Expected outcome: dir1/xyzy exists
            let mut pb = PathBuf::new();
            pb.push(DIR);
            pb.push(FILENAME_IN_HEADER);
            let meta1 = tokio::fs::metadata(&pb).await.unwrap();
            assert!(meta1.is_file() && meta1.len() == 0);
            Ok(())
        })
        .await
        .unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dest_is_broken_link() {
        LitterTray::try_with_async(async |tray| {
            let header = setup(tray).unwrap();
            let _f = TokioFile::create_or_truncate(BROKEN_LINK, &header).await?;
            // Expected outcome: file98 (broken_link_dest) exists
            let meta1 = tokio::fs::metadata(BROKEN_LINK_DEST).await.unwrap();
            assert!(meta1.is_file() && meta1.len() == 0);
            Ok(())
        })
        .await
        .unwrap();
    }
}
