//! File I/O helpers
// (c) 2024 Ross Younger

use crate::protocol::session::Status;
use futures_util::TryFutureExt as _;
use std::{fs::Metadata, io::ErrorKind, path::Path, path::PathBuf, str::FromStr as _};

/// Opens a local file for reading, returning a filehandle and metadata.
/// Error type is a tuple ready to send as a Status response.
pub(crate) async fn open_file(
    filename: &str,
) -> anyhow::Result<(tokio::fs::File, Metadata), (Status, Option<String>, tokio::io::Error)> {
    let path = Path::new(&filename);

    let fh: tokio::fs::File = tokio::fs::File::open(path)
        .await
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => (Status::FileNotFound, Some(e.to_string()), e),
            ErrorKind::PermissionDenied => (Status::IncorrectPermissions, Some(e.to_string()), e),
            _ => (
                Status::IoError,
                Some(format!("unknown error from File::open: {e}")),
                e,
            ),
        })?;

    let meta = fh
        .metadata()
        .map_err(|e| {
            (
                Status::IoError,
                Some(format!("unable to determine file size: {e}")),
                e,
            )
        })
        .await?;

    Ok((fh, meta))
}

/// Opens a local file for writing, from an incoming `FileHeader`
pub(crate) async fn create_truncate_file(
    path: &str,
    header: &crate::protocol::session::FileHeaderV1,
) -> anyhow::Result<tokio::fs::File> {
    let mut dest_path = PathBuf::from_str(path).unwrap(); // this is marked as infallible
    let dest_meta = tokio::fs::metadata(&dest_path).await;
    if let Ok(meta) = dest_meta {
        // if it's a file, proceed (overwriting)
        if meta.is_dir() {
            dest_path.push(header.filename.clone());
        }
    }

    let file = tokio::fs::File::create(dest_path).await?;
    file.set_len(header.size.0).await?;
    Ok(file)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::path::PathBuf;

    use crate::{protocol::session::FileHeaderV1, util::littertray::LitterTray};

    use super::create_truncate_file;

    const NEW_LEN: u64 = 42;
    const FILE: &str = "file1";
    const FILE_LINK: &str = "file2";
    const DIR: &str = "dir1";
    const DIR_LINK: &str = "dir2";
    const BROKEN_LINK: &str = "file99";
    const BROKEN_LINK_DEST: &str = "file98";
    const FILENAME_IN_HEADER: &str = "xyzy";

    fn setup(tray: &mut LitterTray) -> anyhow::Result<FileHeaderV1> {
        let _ = tray.create_text(FILE, "12345")?;
        let _ = tray.make_symlink(FILE, FILE_LINK)?;
        let _ = tray.make_dir(DIR)?;
        let _ = tray.make_symlink(DIR, DIR_LINK)?;
        let _ = tray.make_symlink(BROKEN_LINK_DEST, BROKEN_LINK);

        Ok(FileHeaderV1 {
            size: serde_bare::Uint(NEW_LEN),
            filename: FILENAME_IN_HEADER.to_string(),
        })
    }

    #[tokio::test]
    async fn dest_is_symlink_to_file() {
        LitterTray::try_with_async(async |tray| {
            let header = setup(tray).unwrap();
            let _f = create_truncate_file(FILE_LINK, &header).await?;
            // Expected outcome: file1 is now truncated, file2 is still a symlink to it.
            let meta1 = tokio::fs::metadata("file1").await.unwrap();
            assert!(meta1.is_file() && meta1.len() == NEW_LEN);
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
            let _f = create_truncate_file(DIR, &header).await?;
            // Expected outcome: dir1/xyzy exists
            let mut pb = PathBuf::new();
            pb.push(DIR);
            pb.push(FILENAME_IN_HEADER);
            let meta1 = tokio::fs::metadata(&pb).await.unwrap();
            assert!(meta1.is_file() && meta1.len() == NEW_LEN);
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dest_is_symlink_to_dir() {
        LitterTray::try_with_async(async |tray| {
            let header = setup(tray).unwrap();
            let _f = create_truncate_file(DIR_LINK, &header).await?;
            // Expected outcome: dir1/xyzy exists
            let mut pb = PathBuf::new();
            pb.push(DIR);
            pb.push(FILENAME_IN_HEADER);
            let meta1 = tokio::fs::metadata(&pb).await.unwrap();
            assert!(meta1.is_file() && meta1.len() == NEW_LEN);
            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn dest_is_broken_link() {
        LitterTray::try_with_async(async |tray| {
            let header = setup(tray).unwrap();
            let _f = create_truncate_file(BROKEN_LINK, &header).await?;
            // Expected outcome: file98 (broken_link_dest) exists
            let meta1 = tokio::fs::metadata(BROKEN_LINK_DEST).await.unwrap();
            assert!(meta1.is_file() && meta1.len() == NEW_LEN);
            Ok(())
        })
        .await
        .unwrap();
    }
}
