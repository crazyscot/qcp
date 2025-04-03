//! Endpoint creation
// (c) 2024-2025 Ross Younger

use crate::{
    config::Configuration,
    protocol::control::ConnectionType,
    transport::ThroughputMode,
    util::{self, Credentials},
};

use anyhow::Result;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::rustls::server::WebPkiClientVerifier;
use quinn::{EndpointConfig, rustls};
use rustls::RootCertStore;
use std::sync::Arc;
use tracing::{Level, span, trace, warn};

/// Creates the QUIC endpoint:
/// * `credentials` are generated locally.
/// * `peer_cert` comes from the control channel server message.
/// * `destination` is the server's address (port from the control channel server message).
pub(crate) fn create_endpoint(
    credentials: &Credentials,
    peer_cert: &[u8], /*CertificateDer<'_>*/
    connection_type: ConnectionType,
    config: &Configuration,
    mode: ThroughputMode,
    server: bool,
) -> Result<(quinn::Endpoint, Option<String>)> {
    let _ = span!(Level::TRACE, "create_endpoint").entered();
    let mut root_store = RootCertStore::empty();
    root_store.add(peer_cert.into())?;

    let (client_config, server_config) = if server {
        let verifier = WebPkiClientVerifier::builder(root_store.into()).build()?;
        let mut tls_config = rustls::ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(credentials.cert_chain(), credentials.keypair.clone_key())?;
        tls_config.max_early_data_size = u32::MAX;
        let qsc = QuicServerConfig::try_from(tls_config)?;
        let mut server_cfg = quinn::ServerConfig::with_crypto(Arc::new(qsc));
        let _ = server_cfg.transport_config(crate::transport::create_config(config, mode)?);

        (None, Some(server_cfg))
    } else {
        let tls_config = Arc::new(
            rustls::ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_client_auth_cert(credentials.cert_chain(), credentials.keypair.clone_key())?,
        );
        let mut client_cfg =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls_config)?));
        let _ = client_cfg.transport_config(crate::transport::create_config(config, mode)?);

        (Some(client_cfg), None)
    };

    trace!("bind & configure socket, port={:?}", config.port);
    let mut socket = util::socket::bind_range_for_family(connection_type, config.port)?;
    let wanted_send = match mode {
        ThroughputMode::Both | ThroughputMode::Tx => Some(Configuration::send_buffer().try_into()?),
        ThroughputMode::Rx => None,
    };
    let wanted_recv = match mode {
        ThroughputMode::Both | ThroughputMode::Rx => Some(Configuration::recv_buffer().try_into()?),
        ThroughputMode::Tx => None,
    };

    let warning = util::socket::set_udp_buffer_sizes(&mut socket, wanted_send, wanted_recv)?
        .inspect(|s| warn!("{s:?}"));

    trace!("create endpoint");
    // SOMEDAY: allow user to specify max_udp_payload_size in endpoint config, to support jumbo frames
    let runtime =
        quinn::default_runtime().ok_or_else(|| anyhow::anyhow!("no async runtime found"))?;
    let mut endpoint =
        quinn::Endpoint::new(EndpointConfig::default(), server_config, socket, runtime)?;
    if let Some(c) = client_config {
        endpoint.set_default_client_config(c);
    }

    Ok((endpoint, warning))
}
