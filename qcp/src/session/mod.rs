//! Session protocol command senders and handlers
// (c) 2024-5 Ross Younger

mod common;

mod get;
mod ls;
mod mkdir;
mod put;
mod set_meta;

pub(crate) use {get::Get, ls::Listing, mkdir::CreateDirectory, put::Put, set_meta::SetMetadata};

#[cfg(feature = "unstable-test-helpers")]
#[allow(unused_imports)] // Selectively exported by qcp::test_helpers
pub(crate) use get::test_shared;

use anyhow::Result;
use async_trait::async_trait;
use indicatif::{MultiProgress, ProgressBar};

use crate::{Parameters, client::CopyJobSpec, config::Configuration, protocol::session::Response};

/// Helper macro for making error returns
///
/// Typically called within a command handler as `error_and_return!(self, SomeError)`.
macro_rules! error_and_return {
    ($obj:expr, $inner:expr) => {
        return crate::session::common::send_error(
            &mut $obj.stream.send,
            &anyhow::Error::from($inner),
        )
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
}

/// Result of a completed request
#[derive(Default, Debug, derive_more::Constructor)]
pub struct RequestResult {
    /// Whether the request was successful
    pub success: bool,
    /// Statistics for the command, if applicable (i.e. for file transfer commands)
    pub stats: CommandStats,
    /// Optional response data from the server.
    ///
    /// This is used for commands that return data which the client processes and may cause further commands,
    /// for example `List` returns directory entries which may cause further `Get` commands.
    pub response: Option<Response>,
}

/// Common structure for session protocol commands
#[async_trait]
pub(crate) trait SessionCommandImpl: Send {
    /// Client side implementation, takes care of sending the command and all its
    /// traffic. Does not return until completion (or error).
    /// Returns the number of payload bytes received.
    async fn send(
        &mut self,
        job: &CopyJobSpec,
        display: MultiProgress,
        filename_width: usize,
        spinner: ProgressBar,
        config: &Configuration,
        params: Parameters,
    ) -> Result<RequestResult>;

    /// Server side implementation, takes care of handling the command and all its
    /// traffic. Does not return until completion (or error).
    ///
    /// If the command has arguments, the object constructor is expected to set them up.
    ///
    /// See also the [`crate::session::common::send_ok`] and [`crate::session::common::send_error`] helpers.
    async fn handle(&mut self, io_buffer_size: u64) -> Result<()>;

    /// Accessor for the stored response data, if there is any.
    /// This takes ownership of the response data; future calls will return None.
    ///
    /// **TODO** This is a temporary wart for testing.
    #[allow(dead_code)]
    fn response(&mut self) -> Option<Response> {
        None
    }
}
