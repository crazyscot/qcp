//! Session protocol command senders and handlers
// (c) 2024-5 Ross Younger

mod common;
pub(crate) mod factory;
pub(crate) mod handler;

mod get;
mod ls;
mod mkdir;
mod put;
mod set_meta;

#[cfg(feature = "unstable-test-helpers")]
#[allow(unused_imports)] // Selectively exported by qcp::test_helpers
pub(crate) use get::test_shared;

use anyhow::Result;

use crate::{Parameters, client::CopyJobSpec, protocol::session::ListData};

/// Helper macro for making error returns
///
/// Can be called with either:
/// - `error_and_return!(stream, SomeError)` - for CommandHandler implementations where stream is &mut SendReceivePair
macro_rules! error_and_return {
    ($stream:expr, $inner:expr) => {
        return crate::session::common::send_error(&mut $stream.send, &anyhow::Error::from($inner))
            .await
    };
}
use error_and_return; // export within this crate

#[derive(Debug, Default, Copy, Clone)]
/// Internal statistics for a completed command
#[allow(unreachable_pub)] // Selectively exported by qcp::test_helpers
pub struct CommandStats {
    /// Total number of payload bytes sent
    pub payload_bytes: u64,
    /// Peak transfer rate observed (in bytes per second); this is not terribly accurate at the moment, particularly on PUT commands
    pub peak_transfer_rate: u64,
    /// Number of files skipped because the destination already had the same size as the source
    pub skipped_files: u64,
}

/// Result of a successfully completed request
#[derive(Debug, derive_more::Constructor)]
pub struct RequestResult {
    /// Statistics for the command, if applicable (i.e. for file transfer commands)
    pub stats: CommandStats,
    /// Optional response data from the server.
    ///
    /// This is used for commands that return data which the client processes and may cause further commands,
    /// for example `List` returns directory entries which may cause further `Get` commands.
    pub list: Option<ListData>,
}

impl Default for RequestResult {
    /// A default successful request result with no stats or response data
    fn default() -> Self {
        Self {
            stats: CommandStats::default(),
            list: None,
        }
    }
}

/// Common structure for session protocol commands
pub(crate) trait SessionCommandImpl: Send {
    /// Client side implementation, takes care of sending the command and all its
    /// traffic. Does not return until completion (or error).
    /// Returns the number of payload bytes received.
    fn send<'a>(
        &'a mut self,
        job: &'a CopyJobSpec,
        params: Parameters,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<RequestResult>> + Send + 'a>>;

    /// Server side implementation, takes care of handling the command and all its
    /// traffic. Does not return until completion (or error).
    ///
    /// If the command has arguments, the object constructor is expected to set them up.
    ///
    /// See also the [`crate::session::common::send_ok`] and [`crate::session::common::send_error`] helpers.
    fn handle<'a>(
        &'a mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>>;
}
