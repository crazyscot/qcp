//! Control protocol message logic
// (c) 2024-2025 Ross Younger

//! Control channel management for the qcp client
// (c) 2024 Ross Younger

use std::cmp::min;
use std::time::Duration;

use anyhow::{Context as _, Result};
use indicatif::MultiProgress;
use quinn::{ConnectionStats, Endpoint};
use serde_bare::Uint;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, Stdin, Stdout};
use tokio::time::timeout;
use tracing::{debug, error, info, trace, warn};

use crate::client::Parameters;
use crate::config::{Configuration, Configuration_Optional, Manager};
use crate::control::create_endpoint;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::control::{
    BANNER, COMPATIBILITY_LEVEL, ClientGreeting, ClientMessage, ClientMessageV1, ClosedownReport,
    ClosedownReportV1, CompatibilityLevel, ConnectionType, OLD_BANNER, ServerFailure,
    ServerGreeting, ServerMessage, ServerMessageV1,
};
use crate::transport::combine_bandwidth_configurations;
use crate::util::{Credentials, TimeFormat};

/// Control channel abstraction
pub(crate) struct ControlChannel<S: SendingStream, R: ReceivingStream> {
    stream: SendReceivePair<S, R>,
    /// The other side's declared compatibility level
    pub compat: CompatibilityLevel,
}

impl SendingStream for Stdout {}
impl ReceivingStream for Stdin {}

/// Creates a channel using the current process stdin/out
///
/// # Caution
/// stdout is usually line-buffered, so you probably need to flush it when sending binary data.
pub(crate) fn stdio_channel() -> ControlChannel<Stdout, Stdin> {
    ControlChannel::new((tokio::io::stdout(), tokio::io::stdin()).into())
}

/// Composite return type for `Channel::run_server()`
pub(crate) struct ServerResult {
    /// Final negotiated configuration
    pub(crate) config: Configuration,
    /// The Quinn endpoint created during the control channel phase
    pub(crate) endpoint: Endpoint,
}

impl<S: SendingStream, R: ReceivingStream> ControlChannel<S, R> {
    pub(crate) fn new(stream: SendReceivePair<S, R>) -> Self {
        Self {
            stream,
            compat: CompatibilityLevel::UNKNOWN,
        }
    }

    async fn send<T: ProtocolMessage>(&mut self, message: T, context: &str) -> Result<()> {
        let send = &mut self.stream.send;
        message
            .to_writer_async_framed(send)
            .await
            .with_context(|| format!("sending {context}"))?;
        send.flush().await?;
        Ok(())
    }

    async fn send_error(&mut self, failure: ServerFailure) -> Result<()> {
        self.send(ServerMessage::Failure(failure), "error").await?;
        Ok(())
    }

    async fn recv<T: ProtocolMessage>(&mut self, context: &str) -> Result<T> {
        T::from_reader_async_framed(&mut self.stream.recv)
            .await
            .with_context(|| format!("receiving {context}"))
    }

    async fn flush(&mut self) -> Result<()> {
        self.stream.send.flush().await?;
        Ok(())
    }

    fn process_compatibility_levels(&mut self, theirs: u16) {
        // FUTURE: We may decide to deprecate older compatibility versions. Handle that here.
        let d = match theirs.cmp(&COMPATIBILITY_LEVEL.into()) {
            std::cmp::Ordering::Less => Some("older"),
            std::cmp::Ordering::Equal => None,
            std::cmp::Ordering::Greater => Some("newer"),
        };
        if let Some(d) = d {
            debug!("Remote compatibility level {theirs} is {d} than ours {COMPATIBILITY_LEVEL}");
        }
        self.compat = min(theirs.into(), COMPATIBILITY_LEVEL);
        debug!("selected compatibility level {}", self.compat);
    }

    // =================================================================================
    // CLIENT

    pub(super) async fn client_exchange_greetings(
        &mut self,
        remote_debug: bool,
    ) -> Result<ServerGreeting> {
        self.send(
            ClientGreeting {
                compatibility: COMPATIBILITY_LEVEL.into(),
                debug: remote_debug,
                extension: 0,
            },
            "client greeting",
        )
        .await?;

        let reply = self.recv::<ServerGreeting>("server greeting").await?;
        self.process_compatibility_levels(reply.compatibility);
        Ok(reply)
    }

    pub(super) async fn client_send_message(
        &mut self,
        credentials: &Credentials,
        connection_type: ConnectionType,
        parameters: &Parameters,
        config: &Configuration_Optional,
    ) -> Result<()> {
        // FUTURE: Select the client message version to send based on server's compatibility level.
        let message = ClientMessage::new(
            credentials,
            connection_type,
            parameters.remote_config,
            config,
        );
        debug!("Our client message: {message}");
        self.send(message, "client message").await
    }

    async fn client_read_server_message(&mut self) -> Result<ServerMessageV1> {
        let message = self.recv::<ServerMessage>("server message").await?;
        trace!("Got server message {message:?}");
        // FUTURE: ServerMessage V2 will require more logic to unpack the message contents.
        let message1 = match message {
            ServerMessage::V1(m) => m,
            ServerMessage::Failure(f) => {
                anyhow::bail!("server sent failure message: {f}");
            }
            ServerMessage::ToFollow => {
                anyhow::bail!("remote or logic error: unpacked unexpected ServerMessage::ToFollow")
            }
        };
        Ok(message1)
    }

    /// Runs the client side of the operation, end-to-end.
    ///
    /// Checks the banner, sends the Client Message, reads the Server Message.
    pub(crate) async fn run_client(
        &mut self,
        credentials: &Credentials,
        connection_type: ConnectionType,
        manager: &mut Manager,
        parameters: &Parameters,
    ) -> Result<ServerMessageV1> {
        trace!("opening control channel");

        // PHASE 1: BANNER CHECK
        self.wait_for_banner().await?;

        // PHASE 2: EXCHANGE GREETINGS
        let remote_greeting = self
            .client_exchange_greetings(parameters.remote_debug)
            .await?;
        debug!("got server greeting {remote_greeting:?}");

        // PHASE 3: EXCHANGE OF MESSAGES
        let working = manager.get::<Configuration_Optional>().unwrap_or_default();
        self.client_send_message(credentials, connection_type, parameters, &working)
            .await?;

        trace!("waiting for server message");
        let message1 = self.client_read_server_message().await?;

        manager.merge_provider(&message1);
        manager.apply_system_default(); // SOMEDAY: If we split config into two (bandwidth & options) this shouldn't be necessary.

        if !message1.warning.is_empty() {
            warn!("Remote endpoint warning: {}", &message1.warning);
        }
        Ok(message1)
    }

    pub(super) async fn wait_for_banner(&mut self) -> Result<()> {
        let mut buf = [0u8; BANNER.len()];
        let recv = &mut self.stream.recv;
        let mut reader = recv.take(buf.len() as u64);

        // On entry, we cannot tell whether ssh might be attempting to interact with the user's tty.
        // Therefore we cannot apply a timeout until we have at least one byte through.
        // (Edge case: We cannot currently detect the case where the remote process starts but sends no banner.)

        let n = reader
            .read_exact(&mut buf[0..1])
            .await
            .context("failed to connect control channel")?;
        anyhow::ensure!(n == 1, "control channel closed unexpectedly");

        // Now we have a character, apply a timeout to read the rest.
        // It's hard to imagine a process not sending all of the banner in a single packet, so we'll keep this short.
        let _ = timeout(Duration::from_secs(1), reader.read_exact(&mut buf[1..]))
            .await
            // outer failure means we timed out:
            .context("timed out reading server banner")?
            // inner failure is some sort of I/O error or unexpected eof
            .context("error reading control channel")?;

        let read_banner = std::str::from_utf8(&buf).context("garbage server banner")?;
        match read_banner {
            BANNER => (),
            OLD_BANNER => {
                anyhow::bail!("unsupported protocol version (upgrade server to qcp 0.3.0 or later)")
            }
            b => anyhow::bail!(
                "unsupported protocol version (unrecognised server banner `{}'; may be too new for me?)",
                &b[0..b.len() - 1]
            ),
        }
        Ok(())
    }

    /// Retrieves the closedown report
    pub(crate) async fn read_closedown_report(&mut self) -> Result<ClosedownReportV1> {
        let stats = self.recv::<ClosedownReport>("closedown report").await?;
        // FUTURE: ClosedownReport V2 will require more logic to unpack the message contents.
        let ClosedownReport::V1(stats) = stats else {
            anyhow::bail!("server sent unknown ClosedownReport message type");
        };

        debug!("remote reported stats: {:?}", stats);

        Ok(stats)
    }

    // =================================================================================
    // SERVER

    async fn server_exchange_greetings(&mut self) -> Result<ClientGreeting> {
        self.send(
            ServerGreeting {
                compatibility: COMPATIBILITY_LEVEL.into(),
                extension: 0,
            },
            "server greeting",
        )
        .await?;

        let reply = self.recv::<ClientGreeting>("client greeting").await?;
        self.process_compatibility_levels(reply.compatibility);
        Ok(reply)
    }

    async fn server_read_client_message(&mut self) -> Result<ClientMessageV1> {
        let client_message = match self.recv::<ClientMessage>("client message").await {
            Ok(cm) => cm,
            Err(e) => {
                self.send_error(ServerFailure::Malformed).await?;
                // try to be helpful if there's a human reading
                error!("{e}");
                anyhow::bail!(
                    "In server mode, this program expects to receive a binary data packet on stdin"
                );
            }
        };

        trace!("waiting for client message");
        let message1 = match client_message {
            ClientMessage::V1(m) => m,
            ClientMessage::ToFollow => {
                self.send_error(ServerFailure::Malformed).await?;
                anyhow::bail!("remote or logic error: unpacked unexpected ClientMessage::ToFollow")
            }
        };
        Ok(message1)
    }

    async fn server_send_message(
        &mut self,
        port: u16,
        credentials: &Credentials,
        config: &Configuration,
        warning: String,
    ) -> Result<()> {
        // FUTURE: When later versions of ServerMessage are created, check client compatibility and send the appropriate version.
        self.send(
            ServerMessage::V1(ServerMessageV1 {
                port,
                cert: credentials.certificate.to_vec(),
                name: credentials.hostname.clone(),
                bandwidth_to_server: Uint(config.rx()),
                bandwidth_to_client: Uint(config.tx()),
                rtt: config.rtt,
                congestion: config.congestion.into(),
                initial_congestion_window: Uint(config.initial_congestion_window.into()),
                timeout: config.timeout,
                warning,
                extension: 0,
            }),
            "server message",
        )
        .await?;
        self.flush().await?;
        Ok(())
    }

    fn server_trace_level(debug: bool) -> &'static str {
        if debug { "debug" } else { "info" }
    }

    pub(crate) async fn run_server<
        F: FnOnce(
            &str,
            Option<&MultiProgress>,
            Option<&String>,
            TimeFormat,
            bool,
        ) -> anyhow::Result<()>,
    >(
        &mut self,
        remote_ip: Option<String>,
        manager: &mut Manager,
        setup_tracing: F,
        colours: bool,
    ) -> anyhow::Result<ServerResult> {
        // PHASE 1: BANNER (checked by client)
        self.stream.send.write_all(BANNER.as_bytes()).await?;

        // PHASE 2: GREETINGS
        let remote_greeting = self.server_exchange_greetings().await?;
        let time_format = manager.get_config_field::<TimeFormat>(
            "time_format",
            Some(Configuration::system_default().time_format),
        )?;

        // to provoke a config error here, set RUST_LOG=.
        let level = Self::server_trace_level(remote_greeting.debug);
        setup_tracing(level, None, None, time_format, colours)?;
        // Now we can use the tracing system!

        let _span = tracing::error_span!("Server").entered();
        debug!("got client greeting {remote_greeting:?}");
        debug!("client IP is {}", remote_ip.as_ref().map_or("none", |v| v));

        // PHASE 3: MESSAGES
        // PHASE 3A: Read client message
        let message1 = self.server_read_client_message().await?;

        // PHASE 3B: Process client message
        debug!(
            "got client cert length {}, using {:?}",
            message1.cert.len(),
            message1.connection_type,
        );
        //debug!("client msg {message1:?}");
        if message1.show_config {
            info!(
                "Static configuration:\n{}",
                manager.to_display_adapter::<Configuration>()
            );
        }

        let config = match combine_bandwidth_configurations(manager, &message1) {
            Ok(cfg) => cfg,
            Err(e) => {
                self.send_error(ServerFailure::NegotiationFailed(format!("{e}")))
                    .await?;
                anyhow::bail!("Config negotiation failed: {e}");
            }
        };

        if message1.show_config {
            info!(
                "Final configuration:\n{}",
                manager.to_display_adapter::<Configuration>()
            );
        }

        // PHASE 3C: Create the QUIC endpoint
        let credentials = Credentials::generate()?;
        let (endpoint, warning) = match create_endpoint(
            &credentials,
            &message1.cert,
            message1.connection_type,
            &config,
            // we have no way to know what the client will request, so must configure for both
            crate::transport::ThroughputMode::Both,
            true,
        ) {
            Ok(t) => t,
            Err(e) => {
                self.send_error(ServerFailure::EndpointFailed(format!("{e}")))
                    .await?;
                anyhow::bail!("failed to create server endpoint: {e}");
            }
        };
        let local_addr = endpoint.local_addr()?;
        debug!("Local endpoint address is {local_addr}");

        // PHASE 3D: Send server message
        self.server_send_message(
            local_addr.port(),
            &credentials,
            &config,
            warning.unwrap_or_default(),
        )
        .await?;

        Ok(ServerResult { config, endpoint })
    }

    pub(crate) async fn send_closedown_report(&mut self, stats: &ConnectionStats) -> Result<()> {
        // FUTURE: When later versions of ClosedownReport are created, check client compatibility and send the appropriate version.
        self.send(
            ClosedownReport::V1(ClosedownReportV1::from(stats)),
            "closedown report",
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::{
        client::Parameters,
        config::{Configuration_Optional, Manager},
        control::ControlChannel,
        protocol::{
            common::{
                MessageHeader, ProtocolMessage as _, ReceivingStream, SendReceivePair,
                SendingStream,
            },
            control::{ClosedownReportV1, ConnectionType, OLD_BANNER, ServerMessageV1},
        },
        util::{Credentials, PortRange, TimeFormat, test_protocol::test_plumbing},
    };
    use anyhow::Result;
    use indicatif::MultiProgress;
    use quinn::ConnectionStats;
    use tokio::io::AsyncWriteExt;

    #[allow(clippy::unnecessary_wraps)]
    fn mock_setup_tracing(
        _trace_level: &str,
        _display: Option<&MultiProgress>,
        _filename: Option<&String>,
        _time_format: TimeFormat,
        _colour: bool,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    struct TestClient<S: SendingStream, R: ReceivingStream> {
        creds: Credentials,
        manager: Manager,
        params: Parameters,
        client: ControlChannel<S, R>,
    }
    impl<S: SendingStream, R: ReceivingStream> TestClient<S, R> {
        fn new(pipe: SendReceivePair<S, R>) -> TestClient<S, R> {
            Self {
                creds: Credentials::generate().unwrap(),
                manager: Manager::without_files(None),
                params: Parameters::default(),
                client: ControlChannel::new(pipe),
            }
        }
        // convenience constructor, creates a manager and runs a provided closure on it
        fn with_prefs<F: FnOnce(&mut Manager)>(
            pipe: SendReceivePair<S, R>,
            f: F,
        ) -> TestClient<S, R> {
            let mut rv = Self::new(pipe);
            f(&mut rv.manager);
            rv
        }
        fn go(&mut self) -> impl Future<Output = Result<ServerMessageV1>> {
            self.client.run_client(
                &self.creds,
                ConnectionType::Ipv4,
                &mut self.manager,
                &self.params,
            )
        }
    }

    #[tokio::test]
    async fn happy_path() {
        let (pipe1, pipe2) = test_plumbing();

        let mut cli = TestClient::new(pipe1);
        cli.params.remote_config = true;
        let cli_fut = cli.go();

        let mut server = ControlChannel::new(pipe2);
        let mut manager = Manager::without_files(None);
        let ser_fut = server.run_server(None, &mut manager, mock_setup_tracing, false);

        let (cli_res, ser_res) = tokio::join!(cli_fut, ser_fut);
        assert!(cli_res.is_ok());
        assert!(ser_res.is_ok());

        let stats = ConnectionStats::default();
        let expected = ClosedownReportV1::from(&stats);
        let _ = server.send_closedown_report(&stats).await;
        let got = cli.client.read_closedown_report().await.unwrap();
        assert_eq!(expected, got);
    }

    #[tokio::test]
    async fn old_banner() {
        let (pipe1, mut pipe2) = test_plumbing();
        let mut cli = TestClient::new(pipe1);
        let cli_fut = cli.go();
        pipe2.send.write_all(OLD_BANNER.as_bytes()).await.unwrap();
        let res = cli_fut.await;
        assert!(res.is_err_and(|e| {
            e.to_string()
                .contains("unsupported protocol version (upgrade")
        }));
    }

    #[tokio::test]
    async fn banner_junk() {
        let (pipe1, mut pipe2) = test_plumbing();
        let mut cli = TestClient::new(pipe1);
        let cli_fut = cli.go();
        pipe2
            .send
            .write_all("qqqqqqqqqqqqqqqqq\n".as_bytes())
            .await
            .unwrap();
        let res = cli_fut.await;
        assert!(res.is_err_and(|e| e.to_string().contains("unrecognised server banner")));
    }

    fn fake_cli_with_port(begin: u16, end: u16) -> Configuration_Optional {
        Configuration_Optional {
            port: Some(PortRange { begin, end }),
            remote_port: Some(PortRange { begin, end }),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn negotiation_fails() {
        let (pipe1, pipe2) = test_plumbing();

        let mut cli = TestClient::with_prefs(pipe1, |mgr| {
            mgr.merge_provider(fake_cli_with_port(11111, 11111));
        });
        let cli_fut = cli.go();

        let mut server = ControlChannel::new(pipe2);
        let mut manager = Manager::without_files(None);
        // non-overlapping port range, will fail to negotiate
        manager.merge_provider(fake_cli_with_port(22222, 22222));
        let ser_fut = server.run_server(None, &mut manager, mock_setup_tracing, false);

        let (cli_res, ser_res) = tokio::join!(cli_fut, ser_fut);
        assert!(cli_res.is_err_and(|e| e.to_string().contains("Negotiation Failed")));
        assert!(ser_res.is_err_and(|e| e.to_string().contains("negotiation failed")));
    }

    #[tokio::test]
    async fn client_message_junk() {
        let (mut pipe1, pipe2) = test_plumbing();

        let mut server = ControlChannel::new(pipe2);
        let fut = server.server_read_client_message();
        let write_fut = pipe1.send.write_all(&[255u8; 1024]);

        let (ser_res, write_res) = tokio::join!(fut, write_fut);
        assert!(write_res.is_ok());
        assert!(ser_res.is_err_and(|e| {
            e.to_string()
                .contains("this program expects to receive a binary data packet")
        }));
    }

    #[tokio::test]
    async fn client_message_illegal() {
        let (mut pipe1, pipe2) = test_plumbing();

        let mut server = ControlChannel::new(pipe2);
        let fut = server.server_read_client_message();
        // cook up an illegal (unserializable) framed packet..
        let mut body = vec![0u8];
        let mut packet = MessageHeader { size: 1 }.to_vec().unwrap();
        packet.append(&mut body);
        let fut2 = pipe1.send.write_all(&packet);

        let (res1, res2) = tokio::join!(fut, fut2);
        assert!(res2.is_ok());
        assert!(res1.is_err_and(|e| e.to_string().contains("unexpected ClientMessage::ToFollow")));
    }

    #[tokio::test]
    async fn endpoint_fails() {
        // This is a very unexpected case. The most ready way we've got to simulate it is by presenting an unparseable client certificate.

        async fn broken_client<S: SendingStream, R: ReceivingStream>(
            cli: &mut TestClient<S, R>,
        ) -> Result<ServerMessageV1> {
            let mut bad_creds = Credentials::generate()?;
            bad_creds.certificate = vec![1u8; 256].into();
            cli.client.wait_for_banner().await?;
            let _ = cli.client.client_exchange_greetings(false).await?;
            let manager = Manager::without_files(None);
            let cfg = manager.get::<Configuration_Optional>().unwrap();
            cli.client
                .client_send_message(
                    &bad_creds,
                    ConnectionType::Ipv4,
                    &Parameters::default(),
                    &cfg,
                )
                .await?;
            cli.client.client_read_server_message().await
        }
        let (pipe1, pipe2) = test_plumbing();

        let mut cli = TestClient::new(pipe1);

        let cli_fut = broken_client(&mut cli);

        let mut server = ControlChannel::new(pipe2);
        let mut manager = Manager::without_files(None);
        let ser_fut = server.run_server(None, &mut manager, mock_setup_tracing, false);

        let (cli_res, _) = tokio::join!(cli_fut, ser_fut);
        let e = cli_res.unwrap_err();
        eprintln!("msg {e:?}");
        assert!(e.to_string().contains("Endpoint Failed"));
    }
}
