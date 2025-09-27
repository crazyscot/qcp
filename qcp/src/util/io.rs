//! File & async I/O helpers
// (c) 2024-5 Ross Younger

use crate::protocol::session::Status;
use std::{io::ErrorKind, pin::Pin};
use tokio::io::AsyncRead;

pub(crate) fn status_from_error(e: &tokio::io::Error) -> (Status, Option<String>) {
    match e.kind() {
        ErrorKind::NotFound => (Status::FileNotFound, None),
        ErrorKind::PermissionDenied => (Status::IncorrectPermissions, None),
        _ => (Status::IoError, Some(e.to_string())),
    }
}

pub(crate) async fn read_available_non_blocking<R: AsyncRead + Unpin>(
    mut reader: R,
    buffer: &mut tokio::io::ReadBuf<'_>,
) -> Result<(), std::io::Error> {
    std::future::poll_fn(|cx| {
        // Attempt to read data. If no data is available, poll_read returns Poll::Pending.
        // The Waker in 'cx' will be registered to wake this task when data is ready.
        Pin::new(&mut reader).poll_read(cx, buffer)
    })
    .await
}
