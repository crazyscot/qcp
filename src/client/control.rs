//! Control channel management for the qcp client
// (c) 2024 Ross Younger

use std::cmp::min;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::BufReader;
use tokio::process::{Child, Command};

use anyhow::{anyhow, Context as _, Result};
use indicatif::MultiProgress;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt as _},
    time::timeout,
};
use tracing::{debug, trace, warn};

use crate::config::{Configuration, Configuration_Optional, Manager};
use crate::protocol::common::ProtocolMessage as _;
use crate::protocol::control::{
    ClientGreeting, ClientMessage, ClosedownReport, ClosedownReportV1, CompatibilityLevel,
    ConnectionType, ServerGreeting, ServerMessage, ServerMessageV1, BANNER, COMPATIBILITY_LEVEL,
    OLD_BANNER,
};
use crate::util::Credentials;

use super::Parameters;

/// Control channel abstraction
#[derive(Debug)]
pub struct Channel {
    process: Child,
    /// The server's declared compatibility level
    pub compat: CompatibilityLevel,
    /// The handshaking message sent by the server
    pub message: ServerMessageV1,
}

impl Drop for Channel {
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

impl Channel {
    /// A reasonably controlled shutdown.
    /// (If you want to be rough, simply drop the [`Channel`].)
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
        manager: &Manager,
        parameters: &Parameters,
    ) -> Result<Channel> {
        trace!("opening control channel");

        // PHASE 1: BANNER CHECK

        let mut new1 = Self::launch(display, manager, parameters, remote_host, connection_type)?;
        new1.wait_for_banner().await?;

        // PHASE 2: EXCHANGE GREETINGS

        let mut server_output = new1
            .process
            .stdout
            .as_mut()
            .ok_or(anyhow!("could not access process stdout (can't happen?)"))?;

        let server_input = new1
            .process
            .stdin
            .as_mut()
            .ok_or(anyhow!("could not access process stdin (can't happen?)"))?;

        ClientGreeting {
            compatibility: COMPATIBILITY_LEVEL.into(),
            debug: parameters.remote_debug,
            extension: 0,
        }
        .to_writer_async_framed(server_input)
        .await
        .with_context(|| "error writing client greeting")?;

        let remote_greeting = ServerGreeting::from_reader_async_framed(&mut server_output)
            .await
            .with_context(|| "error reading server greeting")?;

        debug!("got server greeting {remote_greeting:?}");

        // FUTURE: We may decide to deprecate older compatibility versions. Check/handle that here.
        new1.compat = min(remote_greeting.compatibility.into(), COMPATIBILITY_LEVEL);
        debug!("selected compatibility level {}", new1.compat);

        // PHASE 3: EXCHANGE OF MESSAGES

        // FUTURE: Select the client message version to send based on server's compatibility level.
        ClientMessage::new(credentials, connection_type, manager)
            .to_writer_async_framed(server_input)
            .await
            .with_context(|| "error writing client message")?;

        trace!("waiting for server message");
        let message = ServerMessage::from_reader_async_framed(&mut server_output)
            .await
            .inspect_err(|e| eprintln!("{e}"))
            .with_context(|| "error reading server message")?;

        trace!("Got server message {message:?}");
        // FUTURE: ServerMessage V2 will require more logic to unpack the message contents.
        let message1 = match message {
            ServerMessage::V1(m) => m,
            ServerMessage::ToFollow => {
                anyhow::bail!("remote or logic error: unpacked unexpected ServerMessage::ToFollow")
            }
        };

        if !message1.warning.is_empty() {
            warn!("Remote endpoint warning: {}", &message1.warning);
        }
        debug!(
            "Remote endpoint network config: {}",
            message1.bandwidth_info
        );
        new1.message = message1;
        Ok(new1)
    }

    /// This is effectively a constructor. At present, it launches a subprocess.
    fn launch(
        display: &MultiProgress,
        manager: &Manager,
        parameters: &Parameters,
        remote_host: &str,
        connection_type: ConnectionType,
    ) -> Result<Self> {
        let working_config = manager.get::<Configuration_Optional>().unwrap_or_default();
        let defaults = Configuration::system_default();

        let mut server = Command::new(working_config.ssh.unwrap_or_else(|| defaults.ssh.clone()));
        let _ = match connection_type {
            ConnectionType::Ipv4 => server.arg("-4"),
            ConnectionType::Ipv6 => server.arg("-6"),
        };
        let _ = server.args(
            working_config
                .ssh_options
                .unwrap_or_else(|| defaults.ssh_options.clone()),
        );
        let _ = server.args([remote_host, "qcp", "--server"]);
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
        Ok(Self {
            process,
            compat: CompatibilityLevel::UNKNOWN,
            message: ServerMessageV1::default(),
        })
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
        match read_banner {
            BANNER => (),
            OLD_BANNER => {
                anyhow::bail!("unsupported protocol version (upgrade server to qcp 0.3.0 or later)")
            }
            b => anyhow::bail!(
                "unsupported protocol version (unrecognised server banner `{}'; may be too new for me?)",
                &b[0..b.len()-1]
            ),
        }
        Ok(())
    }

    /// Retrieves the closedown report
    pub async fn read_closedown_report(&mut self) -> Result<ClosedownReportV1> {
        let pipe = self
            .process
            .stdout
            .as_mut()
            .ok_or(anyhow!("could not access process stdout (can't happen?)"))?;
        let stats = ClosedownReport::from_reader_async_framed(pipe).await?;
        // FUTURE: ClosedownReport V2 will require more logic to unpack the message contents.
        let ClosedownReport::V1(stats) = stats else {
            anyhow::bail!("server sent unknown ClosedownReport message type");
        };

        debug!("remote reported stats: {:?}", stats);

        Ok(stats)
    }
}
