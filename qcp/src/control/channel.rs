//! Control protocol message logic
// (c) 2024-2025 Ross Younger

//! Control channel management for the qcp client
// (c) 2024 Ross Younger

use std::time::Duration;

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use quinn::{ConnectionStats, Endpoint};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _, Stdin, Stdout};
use tokio::time::timeout;
use tracing::{Instrument as _, debug, error, info, trace, warn};

use crate::client::Parameters;
use crate::config::{Configuration, Configuration_Optional, Manager};
use crate::control::create_endpoint;
use crate::protocol::FindTag as _;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::compat::Feature;
use crate::protocol::control::{
    BANNER, ClientGreeting, ClientMessage, ClientMessage2Attributes, ClientMessageV2,
    ClosedownReport, ClosedownReportV1, Compatibility, CongestionController, ConnectionType,
    Direction, OLD_BANNER, OUR_COMPATIBILITY_LEVEL, OUR_COMPATIBILITY_NUMERIC, ServerFailure,
    ServerGreeting, ServerMessage, ServerMessage2Attributes, ServerMessageV2,
};
use crate::transport::combine_bandwidth_configurations;
use crate::util::{Credentials, TimeFormat, TracingSetupFn};

#[cfg(test)]
use mockall::{automock, predicate::*};

/// Control channel abstraction
#[cfg_attr(test, automock)]
#[async_trait]
pub(crate) trait ControlChannelServerInterface<
    S: SendingStream + 'static,
    R: ReceivingStream + 'static,
>
{
    async fn run_server(
        &mut self,
        remote_ip: Option<String>,
        manager: &mut Manager,
        setup_tracing: TracingSetupFn,
        colours: bool,
        force_compat: Option<Compatibility>,
    ) -> anyhow::Result<ServerResult>;

    async fn run_server_inner(&mut self, manager: &mut Manager) -> anyhow::Result<ServerResult>;

    async fn send_closedown_report(&mut self, stats: &ConnectionStats) -> Result<()>;

    fn compat(&self) -> Compatibility;
}

/// Real control channel
#[derive(Debug)]
pub struct ControlChannel<S: SendingStream, R: ReceivingStream> {
    stream: SendReceivePair<S, R>,
    /// The selected compatibility level for the connection
    pub selected_compat: Compatibility,
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
#[derive(Debug)]
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
            selected_compat: Compatibility::Unknown,
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

    // STATIC METHOD
    fn choose_compatibility_level(ours: u16, theirs: u16) -> Compatibility {
        // Reporting older/newer compatibility levels is useful for debugging.
        use std::cmp::Ordering::{Equal, Greater, Less};
        let (d, result) = match theirs.cmp(&ours) {
            Less => (Some("older"), theirs),
            Equal => (None, ours),
            Greater => (Some("newer"), ours),
        };
        if let Some(d) = d {
            debug!("Remote compatibility level {theirs} is {d} than ours {ours}");
        }
        debug!("Selected compatibility level {result}");
        result.into()
    }

    fn process_compatibility_levels(&mut self, theirs: u16) {
        // FUTURE: We may decide to deprecate older compatibility versions. Handle that here.
        self.selected_compat = Self::choose_compatibility_level(OUR_COMPATIBILITY_NUMERIC, theirs);
    }

    // =================================================================================
    // CLIENT

    async fn client_exchange_greetings(
        &mut self,
        remote_debug: bool,
        force_compat: Option<Compatibility>,
    ) -> Result<ServerGreeting> {
        self.send(
            ClientGreeting {
                compatibility: force_compat.unwrap_or(OUR_COMPATIBILITY_LEVEL).into(),
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

    async fn client_send_message(
        &mut self,
        credentials: &Credentials,
        connection_type: ConnectionType,
        parameters: &Parameters,
        config: &Configuration_Optional,
        direction: Direction,
    ) -> Result<()> {
        let congestion = config
            .congestion
            .unwrap_or(Configuration::system_default().congestion);
        if congestion == CongestionController::NewReno {
            anyhow::ensure!(
                self.selected_compat.supports(Feature::NEW_RENO),
                "Remote host does not support NewReno"
            );
        }

        let tagged_creds =
            credentials.to_tagged_data(self.selected_compat, config.tls_auth_type)?;
        let mut message = ClientMessage::new(
            self.selected_compat,
            tagged_creds,
            connection_type,
            parameters.remote_config,
            config,
        );
        message.set_direction(direction);
        debug!("Our client message: {{ {message} }}");
        self.send(message, "client message").await
    }

    async fn client_read_server_message(&mut self) -> Result<ServerMessageV2> {
        let message = self.recv::<ServerMessage>("server message").await?;
        debug!("Received server message: {{ {message} }}");
        Ok(match message {
            ServerMessage::V1(m) => m.into(),
            ServerMessage::V2(m) => m,
            ServerMessage::Failure(f) => {
                anyhow::bail!("server sent failure message: {f}");
            }
            ServerMessage::ToFollow => {
                anyhow::bail!("remote or logic error: unpacked unexpected ServerMessage::ToFollow")
            }
        })
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
        direction: Direction,
        force_compat: Option<Compatibility>,
    ) -> Result<ServerMessageV2> {
        trace!("opening control channel");

        // PHASE 1: BANNER CHECK
        self.wait_for_banner().await?;

        // PHASE 2: EXCHANGE GREETINGS
        let remote_greeting = self
            .client_exchange_greetings(parameters.remote_debug, force_compat)
            .await?;
        debug!("Received server greeting: {remote_greeting:?}");

        // PHASE 3: EXCHANGE OF MESSAGES
        let working = manager.get::<Configuration_Optional>().unwrap_or_default();
        self.client_send_message(
            credentials,
            connection_type,
            parameters,
            &working,
            direction,
        )
        .await?;

        trace!("waiting for server message");
        let message = self.client_read_server_message().await?;

        manager.merge_provider(&message);
        manager.apply_system_default(); // SOMEDAY: If we split config into two (bandwidth & options) this shouldn't be necessary.
        for attr in &message.attributes {
            if attr.tag() == Some(ServerMessage2Attributes::WarningMessage) {
                warn!(
                    "Remote endpoint warning: {}",
                    attr.data.as_str().unwrap_or("<invalid string>")
                );
            }
        }
        Ok(message)
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

    async fn server_exchange_greetings(
        &mut self,
        force_compat: Option<Compatibility>,
    ) -> Result<ClientGreeting> {
        let compat = force_compat.unwrap_or(OUR_COMPATIBILITY_LEVEL);
        self.send(
            ServerGreeting {
                compatibility: compat.into(),
                extension: 0,
            },
            "server greeting",
        )
        .await?;

        let reply = self.recv::<ClientGreeting>("client greeting").await?;
        self.process_compatibility_levels(reply.compatibility);
        Ok(reply)
    }

    async fn server_read_client_message(&mut self) -> Result<ClientMessageV2> {
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
        let message = match client_message {
            ClientMessage::ToFollow => {
                self.send_error(ServerFailure::Malformed).await?;
                anyhow::bail!("remote or logic error: unpacked unexpected ClientMessage::ToFollow")
            }
            ClientMessage::V1(m) => m.into(),
            ClientMessage::V2(m) => m,
        };
        Ok(message)
    }

    async fn server_send_message(
        &mut self,
        port: u16,
        credentials: &Credentials,
        config: &Configuration,
        warning: String,
    ) -> Result<()> {
        let tagged_creds =
            credentials.to_tagged_data(self.selected_compat, Some(config.tls_auth_type))?;

        let message = ServerMessage::new(
            self.selected_compat,
            config,
            port,
            tagged_creds,
            credentials.hostname.clone(),
            warning,
        );
        debug!("sending server message: {message:?}");
        self.send(message, "server message").await?;
        self.flush().await?;
        Ok(())
    }

    fn server_trace_level(debug: bool) -> &'static str {
        if debug { "debug" } else { "info" }
    }
}

#[async_trait]
impl<S: SendingStream + 'static, R: ReceivingStream + 'static> ControlChannelServerInterface<S, R>
    for ControlChannel<S, R>
{
    async fn run_server(
        &mut self,
        remote_ip: Option<String>,
        manager: &mut Manager,
        setup_tracing: TracingSetupFn,
        colours: bool,
        force_compat: Option<Compatibility>,
    ) -> anyhow::Result<ServerResult> {
        // PHASE 1: BANNER (checked by client)
        self.stream.send.write_all(BANNER.as_bytes()).await?;

        // PHASE 2: GREETINGS
        let remote_greeting = self.server_exchange_greetings(force_compat).await?;
        // server_exchange_greetings sets up self.selected_compat
        let time_format = manager.get_config_field::<TimeFormat>(
            "time_format",
            Some(Configuration::system_default().time_format),
        )?;

        // to provoke a config error here, set RUST_LOG=.
        let level = Self::server_trace_level(remote_greeting.debug);
        setup_tracing(
            level,
            crate::util::ConsoleTraceType::Standard,
            None,
            time_format,
            colours,
        )?;
        // Now we can use the tracing system!
        debug!(
            "client IP is {}",
            remote_ip.as_deref().map_or("none", |v| v)
        );
        debug!("Received client greeting: {remote_greeting:?}");

        self.run_server_inner(manager)
            .instrument(tracing::error_span!("[Server]").or_current())
            .await
    }

    async fn run_server_inner(&mut self, manager: &mut Manager) -> anyhow::Result<ServerResult> {
        // PHASE 3: MESSAGES
        // PHASE 3A: Read client message
        let message2 = self.server_read_client_message().await?;

        // PHASE 3B: Process client message
        debug!("using {:?}", message2.connection_type,);
        debug!("Received client message: {message2}");
        let show_config = message2
            .attributes
            .find_tag(crate::protocol::control::ClientMessage2Attributes::OutputConfig)
            .is_some();
        if show_config {
            info!(
                "Static configuration:\n{}",
                manager.to_display_adapter::<Configuration>()
            );
        }

        let config = match combine_bandwidth_configurations(manager, &message2) {
            Ok(cfg) => cfg,
            Err(e) => {
                self.send_error(ServerFailure::NegotiationFailed(format!("{e}")))
                    .await?;
                anyhow::bail!("Config negotiation failed: {e}");
            }
        };

        if show_config {
            info!(
                "Final configuration:\n{}",
                manager.to_display_adapter::<Configuration>()
            );
        }

        // PHASE 3C: Create the QUIC endpoint
        let credentials = Credentials::generate()?;
        let direction = Direction::from(
            message2
                .attributes
                .find_tag(ClientMessage2Attributes::DirectionOfTravel),
        );
        trace!("Direction of travel: {direction}");

        let (endpoint, warning) = match create_endpoint(
            &credentials,
            &message2.credentials,
            message2.connection_type,
            &config,
            direction.server_mode(),
            true,
            self.selected_compat,
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

    async fn send_closedown_report(&mut self, stats: &ConnectionStats) -> Result<()> {
        // FUTURE: When later versions of ClosedownReport are created, check client compatibility and send the appropriate version.
        self.send(
            ClosedownReport::V1(ClosedownReportV1::from(stats)),
            "closedown report",
        )
        .await?;
        Ok(())
    }

    fn compat(&self) -> Compatibility {
        self.selected_compat
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::{
        client::Parameters,
        config::{Configuration_Optional, Manager},
        control::{ControlChannel, ControlChannelServerInterface as _},
        protocol::{
            common::{
                MessageHeader, ProtocolMessage as _, ReceivingStream, SendReceivePair,
                SendingStream,
            },
            control::{
                ClosedownReportV1, Compatibility, CongestionController, ConnectionType, OLD_BANNER,
                ServerMessageV2,
            },
            test_helpers::new_test_plumbing,
        },
        util::{Credentials, PortRange, TimeFormat},
    };
    use anyhow::Result;
    use pretty_assertions::assert_eq;
    use quinn::ConnectionStats;
    use tokio::io::AsyncWriteExt;

    #[allow(clippy::unnecessary_wraps)]
    fn setup_tracing_stub(
        _trace_level: &str,
        _display: crate::util::ConsoleTraceType,
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
        compat: Compatibility,
    }
    impl<S: SendingStream, R: ReceivingStream> TestClient<S, R> {
        fn new(pipe: SendReceivePair<S, R>, compat: Compatibility) -> TestClient<S, R> {
            Self {
                creds: Credentials::generate().unwrap(),
                manager: Manager::without_files(None),
                params: Parameters::default(),
                client: ControlChannel::new(pipe),
                compat,
            }
        }
        /// convenience constructor, creates a manager and runs a provided closure on it
        fn with_prefs<F: FnOnce(&mut Manager)>(
            pipe: SendReceivePair<S, R>,
            f: F,
            compat: Compatibility,
        ) -> TestClient<S, R> {
            let mut rv = Self::new(pipe, compat);
            f(&mut rv.manager);
            rv
        }
        /// Convenience wrapper, runs the test client (async)
        fn run(&mut self) -> impl Future<Output = Result<ServerMessageV2>> {
            self.client.run_client(
                &self.creds,
                ConnectionType::Ipv4,
                &mut self.manager,
                &self.params,
                crate::protocol::control::Direction::Both,
                Some(self.compat),
            )
        }
    }

    // TODO: Cross-compiled mingw code fails here in quinn::Endpoint::new
    // with Endpoint Failed: OS Error 10045 (FormatMessageW() returned error 317) (os error 10045)
    // Don't run this test on such cross builds for now.
    async fn happy_path(compat: Compatibility) {
        let (pipe1, pipe2) = new_test_plumbing();
        let mut cli = TestClient::new(pipe1, compat);
        cli.params.remote_config = true;
        let cli_fut = cli.run();

        let mut server = ControlChannel::new(pipe2);
        let mut manager = Manager::without_files(None);
        let ser_fut =
            server.run_server(None, &mut manager, setup_tracing_stub, false, Some(compat));

        let (cli_res, ser_res) = tokio::join!(cli_fut, ser_fut);
        eprintln!("Client: {cli_res:?}\nServer: {ser_res:?}");
        assert!(cli_res.is_ok());
        assert!(ser_res.is_ok());

        let stats = ConnectionStats::default();
        let expected = ClosedownReportV1::from(&stats);
        let _ = server.send_closedown_report(&stats).await;
        let got = cli.client.read_closedown_report().await.unwrap();
        assert_eq!(expected, got);
    }

    #[cfg_attr(cross_target_mingw, ignore)] // see comment under happy_path() for why
    #[tokio::test]
    async fn happy_path_compat_1() {
        happy_path(Compatibility::Level(1)).await;
    }

    #[cfg_attr(cross_target_mingw, ignore)] // see comment under happy_path() for why
    #[tokio::test]
    async fn happy_path_compat_3() {
        happy_path(Compatibility::Level(3)).await;
    }

    #[tokio::test]
    async fn old_banner() {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let mut cli = TestClient::new(pipe1, Compatibility::Level(1));
        let cli_fut = cli.run();
        pipe2.send.write_all(OLD_BANNER.as_bytes()).await.unwrap();
        let res = cli_fut.await;
        assert!(res.is_err_and(|e| {
            e.to_string()
                .contains("unsupported protocol version (upgrade")
        }));
    }

    #[tokio::test]
    async fn banner_junk() {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let mut cli = TestClient::new(pipe1, Compatibility::Level(1));
        let cli_fut = cli.run();
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
        let (pipe1, pipe2) = new_test_plumbing();

        let mut cli = TestClient::with_prefs(
            pipe1,
            |mgr| {
                mgr.merge_provider(fake_cli_with_port(11111, 11111));
            },
            Compatibility::Level(1),
        );
        let cli_fut = cli.run();

        let mut server = ControlChannel::new(pipe2);
        let mut manager = Manager::without_files(None);
        // non-overlapping port range, will fail to negotiate
        manager.merge_provider(fake_cli_with_port(22222, 22222));
        let ser_fut = server.run_server(
            None,
            &mut manager,
            setup_tracing_stub,
            false,
            Some(Compatibility::Level(1)),
        );

        let (cli_res, ser_res) = tokio::join!(cli_fut, ser_fut);
        assert!(cli_res.is_err());
        assert!(cli_res.is_err_and(|e| e.to_string().contains("Negotiation Failed")));
        assert!(ser_res.is_err());
        assert!(ser_res.is_err_and(|e| e.to_string().contains("negotiation failed")));
    }

    #[tokio::test]
    async fn client_message_junk() {
        let (mut pipe1, pipe2) = new_test_plumbing();

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
        let (mut pipe1, pipe2) = new_test_plumbing();

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

    #[test]
    fn compatibility_level_comparison() {
        type Uut = ControlChannel<tokio::io::Stdout, tokio::io::Stdin>;
        let cases = &[(1u16, 1u16, 1u16), (1, 2, 1), (2, 1, 1), (65535, 1, 1)];
        for (a, b, result) in cases {
            assert_eq!(
                Uut::choose_compatibility_level(*a, *b),
                (*result).into(),
                "case: {a} {b} -> {result}"
            );
        }
    }

    #[tokio::test]
    async fn compat_check_newreno() {
        let (pipe1, pipe2) = new_test_plumbing();
        // Client runs at compat level 3
        let mut cli = TestClient::new(pipe1, Compatibility::Level(3));
        // ...crucial: set NewReno in the config
        let cfg = Configuration_Optional {
            congestion: Some(CongestionController::NewReno),
            ..Default::default()
        };
        cli.manager.merge_provider(cfg);
        let cli_fut = cli.run();

        let mut server = ControlChannel::new(pipe2);
        let mut manager = Manager::without_files(None);
        // Server runs at compat level 1 i.e. does NOT support NewReno
        let ser_fut = server.run_server(
            None,
            &mut manager,
            setup_tracing_stub,
            false,
            Some(Compatibility::Level(1)),
        );

        let res = tokio::try_join!(cli_fut, ser_fut).unwrap_err();
        assert!(res.to_string().contains("does not support NewReno"));
    }
}
