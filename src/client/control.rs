//! Control channel management for the qcp client
// (c) 2024 Ross Younger

use std::{process::Stdio, time::Duration};

use anyhow::{anyhow, Context as _, Result};
use indicatif::MultiProgress;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt as _, BufReader},
    time::timeout,
};
use tracing::{debug, trace, warn};

use crate::{
    config::Configuration,
    protocol::control::{ClientMessage, ClosedownReport, ConnectionType, ServerMessage, BANNER},
    util::Credentials,
};

use super::Parameters;

/// Control channel abstraction
#[derive(Debug)]
pub struct Channel {
    process: tokio::process::Child,
}

impl Channel {
    /// A reasonably controlled shutdown.
    /// (If you want to be rough, simply drop the `ControlChannel`.)
    pub async fn close(&mut self) -> Result<()> {
        // wait() closes the child process stdin
        let _ = self.process.wait().await?;
        Ok(())
    }

    /// Opens the control channel, checks the banner, sends the Client Message, reads the Server Message.
    pub async fn transact(
        credentials: &Credentials,
        remote_host: &str,
        connection_type: ConnectionType,
        display: &MultiProgress,
        config: &Configuration,
        parameters: &Parameters,
    ) -> Result<(Channel, ServerMessage)> {
        trace!("opening control channel");
        let mut new1 = Self::launch(display, config, parameters, remote_host, connection_type)?;
        new1.wait_for_banner().await?;

        let mut pipe = new1
            .process
            .stdin
            .as_mut()
            .ok_or(anyhow!("could not access process stdin (can't happen?)"))?;
        ClientMessage::write(&mut pipe, &credentials.certificate, connection_type)
            .await
            .with_context(|| "writing client message")?;

        let mut server_output = new1
            .process
            .stdout
            .as_mut()
            .ok_or(anyhow!("could not access process stdout (can't happen?)"))?;

        trace!("waiting for server message");
        let message = ServerMessage::read(&mut server_output)
            .await
            .with_context(|| "reading server message")?;

        trace!("Got server message {message:?}");
        if let Some(w) = message.warning.as_ref() {
            warn!("Remote endpoint warning: {w}");
        }
        debug!("Remote endpoint network config: {}", message.bandwidth_info);
        Ok((new1, message))
    }

    /// This is effectively a constructor. At present, it launches a subprocess.
    fn launch(
        display: &MultiProgress,
        config: &Configuration,
        parameters: &Parameters,
        remote_host: &str,
        connection_type: ConnectionType,
    ) -> Result<Self> {
        let mut server = tokio::process::Command::new(&config.ssh);
        let _ = server.kill_on_drop(true);
        let _ = match connection_type {
            ConnectionType::Ipv4 => server.arg("-4"),
            ConnectionType::Ipv6 => server.arg("-6"),
        };
        let _ = server.args(&config.ssh_opt);
        let _ = server.args([
            remote_host,
            "qcp",
            "--server",
            // Remote receive bandwidth = our transmit bandwidth
            "-b",
            &config.tx().to_string(),
            // Remote transmit bandwidth = our receive bandwidth
            "-B",
            &config.rx().to_string(),
            "--rtt",
            &config.rtt.to_string(),
            "--congestion",
            &config.congestion.to_string(),
            "--timeout",
            &config.timeout.to_string(),
        ]);
        if parameters.remote_debug {
            let _ = server.arg("--debug");
        }
        match config.initial_congestion_window {
            0 => (),
            w => {
                let _ = server.args(["--initial-congestion-window", &w.to_string()]);
            }
        }
        if !config.remote_port.is_default() {
            let _ = server.args(["--port", &config.remote_port.to_string()]);
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

    async fn wait_for_banner(&mut self) -> Result<()> {
        let channel = self
            .process
            .stdout
            .as_mut()
            .expect("logic error: missing server stdout");
        let mut buf = [0u8; BANNER.len()];
        let mut reader = channel.take(buf.len() as u64);

        // On entry, we cannot tell whether ssh might be attempting to interact with the user's tty.
        // Therefore we cannot apply a timeout until we have at least one byte through.
        // (Edge case: We cannot currently detect the case where the remote process starts but sends no banner.)

        let n = reader
            .read_exact(&mut buf[0..1])
            .await
            .with_context(|| "failed to connect control channel")?;
        anyhow::ensure!(n == 1, "control channel closed unexpectedly");

        // Now we have a character, apply a timeout to read the rest.
        // It's hard to imagine a process not sending all of the banner in a single packet, so we'll keep this short.
        let _ = timeout(Duration::from_secs(1), reader.read_exact(&mut buf[1..]))
            .await
            // outer failure means we timed out:
            .with_context(|| "timed out reading server banner")?
            // inner failure is some sort of I/O error or unexpected eof
            .with_context(|| "error reading control channel")?;

        let read_banner = std::str::from_utf8(&buf).with_context(|| "garbage server banner")?;
        anyhow::ensure!(BANNER == read_banner, "incompatible server banner");
        Ok(())
    }

    /// Retrieves the closedown report
    pub async fn read_closedown_report(&mut self) -> Result<ClosedownReport> {
        let pipe = self
            .process
            .stdout
            .as_mut()
            .ok_or(anyhow!("could not access process stdout (can't happen?)"))?;
        let stats = ClosedownReport::read(pipe).await?;
        debug!("remote reported stats: {:?}", stats);
        Ok(stats)
    }
}
