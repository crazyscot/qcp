//! Subprocess management (client side)
// (c) 2024-2025 Ross Younger

use std::process::Stdio;

use tokio::process::{ChildStderr, ChildStdin, ChildStdout, Command};

use anyhow::{Result, anyhow};
use tracing::warn;

use crate::protocol::common::SendReceivePair;
use crate::protocol::common::{ReceivingStream, SendingStream};

/// A child process (usually ssh) that implements the connection to the remote
#[derive(Debug, derive_more::Constructor)]
pub(crate) struct ProcessWrapper {
    process: tokio::process::Child,
}

impl Drop for ProcessWrapper {
    fn drop(&mut self) {
        // Tidy up as best we can.
        if let Ok(Some(_)) = self.process.try_wait() {
            return;
        }
        let _ = self
            .process
            .start_kill()
            .map_err(|e| warn!("killing connection process: {e}"));
        let _ = self
            .process
            .try_wait()
            .map_err(|e| warn!("reaping connection process: {e}"));
    }
}

impl SendingStream for ChildStdin {}
impl ReceivingStream for ChildStdout {}

impl ProcessWrapper {
    /// Wraps a [`tokio::process::Command`] with our stream wrapper.
    pub(crate) fn spawn(mut cmd: Command) -> Result<Self> {
        let process = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow!("could not spawn child process: {e}"))?;
        Ok(Self { process })
    }

    /// Accessor for the child process stderr.
    /// Note that this only works once; future calls return None.
    pub(crate) fn stderr(&mut self) -> Option<ChildStderr> {
        self.process.stderr.take()
    }

    /// A reasonably controlled shutdown.
    /// (If you don't mind being rough, simply drop the [`ProcessWrapper`].)
    pub(crate) async fn close(&mut self) -> Result<()> {
        // wait() closes the child process stdin
        let _ = self.process.wait().await?;
        Ok(())
    }

    /// Accessor for the communication channels.
    /// Note that this can only be called once; future calls error.
    pub(crate) fn stream_pair(&mut self) -> Result<SendReceivePair<ChildStdin, ChildStdout>> {
        let sp = SendReceivePair::from((
            self.process
                .stdin
                .take()
                .ok_or_else(|| anyhow!("could not access process stdin"))?,
            self.process
                .stdout
                .take()
                .ok_or_else(|| anyhow!("could not access process stdout"))?,
        ));
        Ok(sp)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    #[cfg(unix)]
    #[tokio::test]
    async fn drop_coverage() {
        let process = tokio::process::Command::new("sleep")
            .arg("100")
            .spawn()
            .expect("could not spawn sleep command");
        let wrapper = super::ProcessWrapper { process };
        drop(wrapper);
    }
}
