//! Control channel management for the qcp client
// (c) 2024 Ross Younger

use std::{net::IpAddr, process::Stdio, time::Duration};

use anyhow::{anyhow, Context as _, Result};
use tokio::{io::AsyncReadExt as _, time::timeout};
use tracing::{debug, trace};

use crate::{
    cert::Credentials,
    cli::CliArgs,
    protocol::control::{ClientMessage, ClosedownReport, ServerMessage, BANNER},
    util::AddressFamily,
};

/// The parameter set needed to set up the control channel
#[derive(Debug)]
pub struct Parameters {
    remote_host: String,
    remote_debug: bool,
    remote_tx_bw_bytes: u64,
    remote_rx_bw_bytes: u64,
    rtt_ms: u16,
    timeout: Duration,
    bbr: bool,
    iwind: Option<u64>,
    family: AddressFamily,
}

impl TryFrom<&CliArgs> for Parameters {
    type Error = anyhow::Error;

    fn try_from(args: &CliArgs) -> std::result::Result<Self, Self::Error> {
        Ok(Self {
            remote_host: args.remote_host()?.to_string(),
            remote_debug: args.remote_debug,
            // Note that we flip inbound and outbound here as we're computing parameters to give to the remote
            remote_rx_bw_bytes: args.bandwidth_outbound_active(),
            remote_tx_bw_bytes: args.bandwidth.size(),
            rtt_ms: args.rtt,
            timeout: args.timeout,
            bbr: args.bbr,
            iwind: args.initial_congestion_window,
            family: args.address_family(),
        })
    }
}

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
    pub async fn transact(
        parameters: &Parameters,
        credentials: &Credentials,
        server_address: IpAddr,
    ) -> Result<(ControlChannel, ServerMessage)> {
        debug!("opening control channel");
        let mut new1 = Self::launch(parameters)?;
        new1.wait_for_banner(parameters.timeout).await?;

        let mut pipe = new1
            .process
            .stdin
            .as_mut()
            .ok_or(anyhow!("could not access process stdin (can't happen?)"))?;
        ClientMessage::write(&mut pipe, &credentials.certificate, server_address.into()).await?;

        let mut server_output = new1
            .process
            .stdout
            .as_mut()
            .ok_or(anyhow!("could not access process stdout (can't happen?)"))?;

        trace!("waiting for server message");
        let message = ServerMessage::read(&mut server_output).await?;
        Ok((new1, message))
    }

    /// This is effectively a constructor. At present, it launches a subprocess.
    fn launch(args: &Parameters) -> Result<Self> {
        let mut server = tokio::process::Command::new("ssh");
        let _ = server.kill_on_drop(true);
        let _ = match args.family {
            AddressFamily::Any => &mut server,
            AddressFamily::IPv4 => server.arg("-4"),
            AddressFamily::IPv6 => server.arg("-6"),
        };
        let _ = server.args([
            &args.remote_host,
            "qcp",
            "--server",
            "-b",
            &args.remote_rx_bw_bytes.to_string(),
            "-B",
            &args.remote_tx_bw_bytes.to_string(),
            "--rtt",
            &args.rtt_ms.to_string(),
        ]);
        if args.remote_debug {
            let _ = server.arg("--debug");
        }
        if args.bbr {
            let _ = server.arg("--bbr");
        }
        if let Some(w) = args.iwind {
            let _ = server.args(["--initial-congestion-window", &w.to_string()]);
        }
        let _ = server
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // TODO: pipe this more nicely, output on error?
            .kill_on_drop(true);
        trace!("spawning command: {:?}", server);
        let process = server
            .spawn()
            .context("Could not launch control connection to remote server")?;
        Ok(Self { process })
    }

    async fn wait_for_banner(&mut self, limit: Duration) -> Result<()> {
        let channel = self.process.stdout.as_mut().expect("missing server stdout");
        let mut buf = [0u8; BANNER.len()];
        let mut reader = channel.take(buf.len() as u64);
        let n_fut = reader.read_exact(&mut buf);

        let n = timeout(limit, n_fut)
            .await
            .with_context(|| "timed out reading server banner")??;

        let read_banner = std::str::from_utf8(&buf).with_context(|| "bad server banner")?;
        anyhow::ensure!(n != 0, "failed to connect"); // the process closed its stdout
        anyhow::ensure!(BANNER == read_banner, "server banner not as expected");
        Ok(())
    }

    pub(crate) async fn read_closedown_report(&mut self) -> Result<ClosedownReport> {
        let pipe = self
            .process
            .stdout
            .as_mut()
            .ok_or(anyhow!("could not access process stdout (can't happen?)"))?;
        ClosedownReport::read(pipe).await
    }
}
