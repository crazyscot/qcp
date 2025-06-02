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

#[derive(Debug, Copy, Clone)]
pub(crate) struct CommandStats {
    pub payload_bytes: u64,
    pub peak_transfer_rate: u64,
}

impl CommandStats {
    pub(crate) fn new() -> Self {
        CommandStats {
            payload_bytes: 0,
            peak_transfer_rate: 0,
        }
    }
    /// Combine this set of stats with another.
    /// Sum payload bytes; peak becomes peak of either.
    pub(crate) fn fold(&mut self, other: CommandStats) {
        self.payload_bytes += other.payload_bytes;
        self.peak_transfer_rate = u64::max(self.peak_transfer_rate, other.peak_transfer_rate);
    }
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
        spinner: ProgressBar,
        config: &Configuration,
        quiet: bool,
    ) -> Result<CommandStats>;

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
    ) -> Result<CommandStats> {
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::CommandStats;

    #[test]
    fn stats() {
        let mut acc = CommandStats::new();
        let d1 = CommandStats {
            payload_bytes: 42,
            peak_transfer_rate: 3456,
        };
        let d2 = CommandStats {
            payload_bytes: 78,
            peak_transfer_rate: 2345,
        };
        acc.fold(d1);
        acc.fold(d2);
        assert_eq!(acc.payload_bytes, 42 + 78);
        assert_eq!(acc.peak_transfer_rate, 3456);
    }
}
