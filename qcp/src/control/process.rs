//! Subprocess management (client side)
// (c) 2024-2025 Ross Younger

use std::process::Stdio;
use tokio::{
    io::BufReader,
    process::{ChildStdin, ChildStdout},
};

use anyhow::{Context as _, Result, anyhow};
use indicatif::MultiProgress;
use tokio::io::AsyncBufReadExt;
use tracing::{debug, warn};

use crate::protocol::control::ConnectionType;
use crate::{client::Parameters, protocol::common::SendReceivePair};
use crate::{
    config::{Configuration, Configuration_Optional, Manager},
    protocol::common::{ReceivingStream, SendingStream},
};

/// A child process (usually ssh) that implements the connection to the remote
#[derive(Debug)]
pub(crate) struct Ssh {
    process: tokio::process::Child,
}

impl Drop for Ssh {
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

impl Ssh {
    /// A reasonably controlled shutdown.
    /// (If you want to be rough, simply drop the [`Channel`].)
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

    /// Constructor
    pub(crate) fn new(
        display: &MultiProgress,
        manager: &Manager,
        parameters: &Parameters,
        ssh_hostname: &str,
        connection_type: ConnectionType,
    ) -> Result<Self> {
        let working_config = manager.get::<Configuration_Optional>().unwrap_or_default();
        let defaults = Configuration::system_default();

        let mut server = tokio::process::Command::new(
            working_config.ssh.unwrap_or_else(|| defaults.ssh.clone()),
        );
        let _ = match connection_type {
            ConnectionType::Ipv4 => server.arg("-4"),
            ConnectionType::Ipv6 => server.arg("-6"),
        };
        let _ = server.args(
            working_config
                .ssh_options
                .unwrap_or_else(|| defaults.ssh_options.clone()),
        );

        // hostname is sent here -------------------------
        let _ = server.args([ssh_hostname]);
        if working_config.ssh_subsystem.unwrap_or(false) {
            let _ = server.args(["-s", "qcp"]);
        } else {
            let _ = server.args(["qcp", "--server"]);
        }

        let _ = server
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        if !parameters.quiet {
            let _ = server.stderr(Stdio::piped());
        } // else inherit
        debug!("spawning command: {:?}", server);
        let mut process = server
            .spawn()
            .context("Could not launch control connection to remote server")?;

        // Whatever the remote outputs, send it to our output in a way that doesn't mess things up.
        if !parameters.quiet {
            let stderr = process.stderr.take();
            let Some(stderr) = stderr else {
                anyhow::bail!("could not get stderr of remote process");
            };
            let cloned = display.clone();
            let _reader = tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    // Calling cloned.println() sometimes messes up; there seems to be a concurrency issue.
                    // But we don't need to worry too much about that. Just write it out.
                    cloned.suspend(|| eprintln!("{line}"));
                }
            });
        }
        Ok(Self { process })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use indicatif::MultiProgress;

    use crate::control::ControlChannel;
    use crate::{client::Parameters, config::Manager};

    use super::Ssh;

    #[tokio::test]
    async fn ssh_no_such_host() {
        let mp = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden());
        let manager = Manager::without_files(None);
        let params = Parameters::default();
        let mut child = Ssh::new(
            &mp,
            &manager,
            &params,
            "no-such-host.invalid",
            crate::protocol::control::ConnectionType::Ipv4,
        )
        .unwrap();

        // There isn't, at present, a signal that the ssh connection failed. But we see it soon enough when we look for the banner.
        let mut control = ControlChannel::new(child.stream_pair().unwrap());
        let e = control.wait_for_banner().await.unwrap_err();
        assert!(e.to_string().contains("failed to connect"));
        child.close().await.unwrap();
        drop(child);
    }
}
