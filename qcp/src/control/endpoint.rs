//! Endpoint creation
// (c) 2024-2025 Ross Younger

use crate::{
    config::Configuration,
    protocol::{
        TaggedData,
        control::{Compatibility, ConnectionType, CredentialsType},
    },
    transport::ThroughputMode,
    util::{self, Credentials},
};

use anyhow::Result;
use num_traits::ToPrimitive;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::rustls::{
    client::{AlwaysResolvesClientRawPublicKeys, ResolvesClientCert, WebPkiServerVerifier},
    server::{AlwaysResolvesServerRawPublicKeys, ResolvesServerCert, WebPkiClientVerifier},
    sign::CertifiedKey,
};
use quinn::{EndpointConfig, rustls};
use rustls::RootCertStore;
use std::sync::Arc;
use tracing::{Level, span, trace, warn};

/// Creates the QUIC endpoint:
/// * `credentials` are generated locally.
/// * `peer_cert` comes from the control channel server message.
/// * `destination` is the server's address (port from the control channel server message).
pub fn create_endpoint(
    credentials: &Credentials,
    peer_cert: &TaggedData<CredentialsType>,
    connection_type: ConnectionType,
    config: &Configuration,
    mode: ThroughputMode,
    server: bool,
    compat: Compatibility,
) -> Result<(quinn::Endpoint, Option<String>)> {
    let _ = span!(Level::TRACE, "create_endpoint").entered();
    let mut root_store = RootCertStore::empty();
    let peer_cert_data = peer_cert.data.as_bytes_ref();
    let Some(peer_cert_data) = peer_cert_data else {
        anyhow::bail!("Invalid peer credentials");
    };
    match peer_cert.tag() {
        Some(CredentialsType::X509) | Some(CredentialsType::Rfc7250) => (),
        Some(CredentialsType::Invalid) | None => {
            anyhow::bail!("Unknown peer credentials type {peer_cert}")
        }
    };
    root_store.add(peer_cert_data.as_slice().into())?;

    let (client_config, server_config) = if server {
        let verifier = WebPkiClientVerifier::builder(root_store.into()).build()?;
        let mut tls_config = if true {
            //### XXX WIP: refactor from here: add creds to control message, then match on it here
            rustls::ServerConfig::builder()
                .with_client_cert_verifier(verifier)
                .with_single_cert(credentials.cert_chain(), credentials.keypair.clone_key())?
        } else {
            /*
            let key = Arc::new(CertifiedKey::new(vec![], credentials.rfc7250.key.clone()));
            let resolver: Arc<dyn ResolvesServerCert> =
                Arc::new(AlwaysResolvesServerRawPublicKeys::new(key));

            rustls::ServerConfig::builder()
                .with_client_cert_verifier(verifier)
                .with_cert_resolver(resolver)
                */
            todo!()
        };
        tls_config.max_early_data_size = u32::MAX;
        let qsc = QuicServerConfig::try_from(tls_config)?;
        let mut server_cfg = quinn::ServerConfig::with_crypto(Arc::new(qsc));
        let _ =
            server_cfg.transport_config(crate::transport::create_config(config, mode, compat)?.0);

        (None, Some(server_cfg))
    } else {
        let tls_config = if true {
            Arc::new(
                rustls::ClientConfig::builder()
                    .with_root_certificates(root_store)
                    .with_client_auth_cert(
                        credentials.cert_chain(),
                        credentials.keypair.clone_key(),
                    )?,
            )
        } else {
            /*
            let verifier = WebPkiServerVerifier::builder(root_store.into()).build()?;
            let key = Arc::new(CertifiedKey::new(vec![], credentials.rfc7250.key.clone()));
            let resolver: Arc<dyn ResolvesClientCert> =
                Arc::new(AlwaysResolvesClientRawPublicKeys::new(key));

            Arc::new(
                rustls::ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(verifier)
                    .with_client_cert_resolver(resolver),
            )
            */
            todo!()
        };
        let mut client_cfg =
            quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls_config)?));
        let _ =
            client_cfg.transport_config(crate::transport::create_config(config, mode, compat)?.0);

        (Some(client_cfg), None)
    };

    trace!("bind & configure socket, port={:?}", config.port);
    let mut socket = util::socket::bind_range_for_family(connection_type, config.port)?;
    let udp_buf = config
        .udp_buffer
        .to_usize()
        .ok_or(anyhow::anyhow!("udp_buffer size overflowed usize"))?;
    let wanted_send = match mode {
        ThroughputMode::Both | ThroughputMode::Tx => Some(udp_buf),
        ThroughputMode::Rx => None,
    };
    let wanted_recv = match mode {
        ThroughputMode::Both | ThroughputMode::Rx => Some(udp_buf),
        ThroughputMode::Tx => None,
    };

    let warning = util::socket::set_udp_buffer_sizes(&mut socket, wanted_send, wanted_recv)?
        .warning
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
