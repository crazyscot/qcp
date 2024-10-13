//! Control channel management for the qcp client
// (c) 2024 Ross Younger

use std::{net::IpAddr, process::Stdio, time::Duration};

use anyhow::{anyhow, Context as _, Result};
use indicatif::MultiProgress;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt as _, BufReader},
    time::timeout,
};
use tracing::{debug, trace, warn};

use crate::{
    cli::CliArgs,
    protocol::control::{ClientMessage, ClosedownReport, ServerMessage, BANNER},
    util::{cert::Credentials, AddressFamily},
};

/// Control channel abstraction
#[derive(Debug)]
#[allow(clippy::module_name_repetitions)]
pub struct ControlChannel {
    process: tokio::process::Child,
}

impl ControlChannel {
    /// A reasonably controlled shutdown.
    /// (If you want to be rough, simply drop the `ControlChannel`.)
    pub async fn close(&mut self) -> Result<()> {
        // wait() closes the child process stdin
        let _ = self.process.wait().await?;
        Ok(())
    }

    /// Opens the control channel, checks the banner, sends the Client Message, reads the Server Message.
    pub(crate) async fn transact(
        credentials: &Credentials,
        server_address: IpAddr,
        display: &MultiProgress,
        args: &CliArgs,
    ) -> Result<(ControlChannel, ServerMessage)> {
        trace!("opening control channel");
        let mut new1 = Self::launch(args, display, args.quiet)?;
        new1.wait_for_banner().await?;

        let mut pipe = new1
            .process
            .stdin
            .as_mut()
            .ok_or(anyhow!("could not access process stdin (can't happen?)"))?;
        ClientMessage::write(&mut pipe, &credentials.certificate, server_address.into())
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
    fn launch(args: &CliArgs, display: &MultiProgress, quiet: bool) -> Result<Self> {
        let mut server = tokio::process::Command::new(&args.ssh);
        let _ = server.kill_on_drop(true);
        let _ = match args.address_family() {
            AddressFamily::Any => &mut server,
            AddressFamily::IPv4 => server.arg("-4"),
            AddressFamily::IPv6 => server.arg("-6"),
        };
        let _ = server.args(&args.ssh_opt);
        let _ = server.args([
            args.remote_user_host()?,
            "qcp",
            "--server",
            // Remote receive bandwidth = our transmit bandwidth
            "-b",
            &args.bandwidth.tx().to_string(),
            // Remote transmit bandwidth = our receive bandwidth
            "-B",
            &args.bandwidth.rx().to_string(),
            "--rtt",
            &args.bandwidth.rtt.to_string(),
            "--congestion",
            &args.bandwidth.congestion.to_string(),
            "--timeout",
            &args.quic.timeout.as_secs().to_string(),
        ]);
        if args.remote_debug {
            let _ = server.arg("--debug");
        }
        if let Some(w) = args.bandwidth.initial_congestion_window {
            let _ = server.args(["--initial-congestion-window", &w.to_string()]);
        }
        if let Some(pr) = args.remote_port {
            let _ = server.args(["--port", &pr.to_string()]);
        }
        let _ = server
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        if !quiet {
            let _ = server.stderr(Stdio::piped());
        } // else inherit
        debug!("spawning command: {:?}", server);
        let mut process = server
            .spawn()
            .context("Could not launch control connection to remote server")?;

        // Whatever the remote outputs, send it to our output in a way that doesn't mess things up.
        if !quiet {
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

    pub(crate) async fn read_closedown_report(&mut self) -> Result<ClosedownReport> {
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
