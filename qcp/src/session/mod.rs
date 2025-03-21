//! Session protocol command senders and handlers
// (c) 2024-5 Ross Younger

mod common;

mod get;
mod put;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test;

pub(crate) use {get::Get, put::Put};

use anyhow::Result;
use async_trait::async_trait;
use indicatif::{MultiProgress, ProgressBar};

use crate::{client::CopyJobSpec, config::Configuration};

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
        spinner: ProgressBar,
        config: &Configuration,
        quiet: bool,
    ) -> Result<u64>;

    /// Server side implementation, takes care of handling the command and all its
    /// traffic. Does not return until completion (or error).
    ///
    /// If the command has arguments, the object constructor is expected to set them up.
    async fn handle(&mut self) -> Result<()>;

    #[cfg_attr(coverage_nightly, coverage(off))]
    #[cfg(test)]
    /// Syntactic sugar for unit tests.
    /// This is a wrapper to send() with some fixed arguments common to testing.
    async fn send_test(
        &mut self,
        spec: &CopyJobSpec,
        config: Option<&Configuration>,
    ) -> Result<u64> {
        let config = config.unwrap_or_else(|| Configuration::system_default());
        self.send(
            spec,
            indicatif::MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden()),
            indicatif::ProgressBar::hidden(),
            config,
            true,
        )
        .await
    }
}
