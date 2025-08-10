//! File I/O helpers
// (c) 2024 Ross Younger

use crate::protocol::session::Status;
use std::io::ErrorKind;

pub(crate) fn status_from_error(e: &tokio::io::Error) -> (Status, Option<String>) {
    match e.kind() {
        ErrorKind::NotFound => (Status::FileNotFound, None),
        ErrorKind::PermissionDenied => (Status::IncorrectPermissions, None),
        _ => (Status::IoError, Some(e.to_string())),
    }
}
