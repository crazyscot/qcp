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
            ErrorKind::Other => (Status::IoError, Some(e.to_string()), e),
            _ => (
                Status::IoError,
                Some(format!("unhandled error from File::open: {e}")),
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
        } else if meta.is_symlink() {
            // TODO: Need to cope with this case; test whether it's a directory?
            let deref = std::fs::read_link(&dest_path)?;
            if std::fs::metadata(deref).is_ok_and(|meta| meta.is_dir()) {
                dest_path.push(header.filename.clone());
            }
            // Else assume the link points to a file, which we will overwrite.
        }
    }

    let file = tokio::fs::File::create(dest_path).await?;
    file.set_len(header.size.0).await?;
    Ok(file)
}
