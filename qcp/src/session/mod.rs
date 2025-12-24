//! Session protocol command senders and handlers
// (c) 2024-5 Ross Younger

mod common;

mod get;
mod mkdir;
mod put;
mod set_meta;

pub(crate) use {get::Get, mkdir::CreateDirectory, put::Put, set_meta::SetMetadata};

#[cfg(feature = "unstable-test-helpers")]
#[allow(unused_imports)] // Selectively exported by qcp::test_helpers
pub(crate) use get::test_shared;

use anyhow::Result;
use async_trait::async_trait;
use indicatif::{MultiProgress, ProgressBar};

use crate::{Parameters, client::CopyJobSpec, config::Configuration};

#[derive(Debug, Default, Copy, Clone)]
/// Internal statistics for a completed command
#[allow(unreachable_pub)] // Selectively exported by qcp::test_helpers
pub struct CommandStats {
    /// Total number of payload bytes sent
    pub payload_bytes: u64,
    /// Peak transfer rate observed (in bytes per second); this is not terribly accurate at the moment, particularly on PUT commands
    pub peak_transfer_rate: u64,
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
    ) -> Result<CommandStats>;

    /// Server side implementation, takes care of handling the command and all its
    /// traffic. Does not return until completion (or error).
    ///
    /// If the command has arguments, the object constructor is expected to set them up.
    async fn handle(&mut self, io_buffer_size: u64) -> Result<()>;
}
