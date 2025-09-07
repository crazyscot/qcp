//! Endpoint creation
// (c) 2024-2025 Ross Younger

use crate::{
    config::Configuration,
    control::{SimpleRpkClientCertVerifier, SimpleRpkServerCertVerifier},
    protocol::{
        TaggedData,
        control::{Compatibility, ConnectionType, CredentialsType},
    },
    transport::ThroughputMode,
    util::{self, Credentials},
};

use anyhow::Result;
use num_traits::ToPrimitive;
use quinn::{
    ClientConfig as QuinnClientConfig, ServerConfig as QuinnServerConfig,
    crypto::rustls::{QuicClientConfig, QuicServerConfig},
};
use quinn::{EndpointConfig, rustls};
use rustls::{
    RootCertStore,
    client::danger::ServerCertVerifier,
    client::{AlwaysResolvesClientRawPublicKeys, WebPkiServerVerifier},
    server::danger::ClientCertVerifier,
    server::{AlwaysResolvesServerRawPublicKeys, WebPkiClientVerifier},
};
use rustls_pki_types::{
    SubjectPublicKeyInfoDer,
    pem::{PemObject, SectionKind},
};
use std::sync::Arc;
use tracing::{Level, span, trace, warn};

/// Creates the QUIC endpoint:
/// * `credentials` are generated locally.
/// * `peer_cert` comes from the control channel server message.
/// * `destination` is the server's address (port from the control channel server message).
pub fn create_endpoint(
    our_creds: &Credentials,
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
    if let Some(CredentialsType::X509) = peer_cert.tag() {
        root_store.add(peer_cert_data.as_slice().into())?;
    }

    let client_cfg = (!server)
        .then(|| client_config(our_creds, peer_cert, compat, config, mode))
        .transpose()?;
    let server_cfg = server
        .then(|| server_config(our_creds, peer_cert, compat, config, mode))
        .transpose()?;

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
        quinn::Endpoint::new(EndpointConfig::default(), server_cfg, socket, runtime)?;
    if let Some(c) = client_cfg {
        endpoint.set_default_client_config(c);
    }
    Ok((endpoint, warning))
}

fn server_config(
    our_creds: &Credentials,
    peer_cert: &TaggedData<CredentialsType>,
    compat: Compatibility,
    config: &Configuration,
    mode: ThroughputMode,
) -> Result<QuinnServerConfig> {
    let peer_cert_data = peer_cert.data.as_bytes_ref();
    let Some(peer_cert_data) = peer_cert_data else {
        anyhow::bail!("Missing peer credentials");
    };

    let verifier: Arc<dyn ClientCertVerifier> = match peer_cert.tag() {
        Some(CredentialsType::X509) => {
            let mut root_store = RootCertStore::empty();
            root_store.add(peer_cert_data.as_slice().into())?;
            WebPkiClientVerifier::builder(root_store.into()).build()?
        }
        Some(CredentialsType::RawPublicKey) => {
            //let cert = CertificateDer::from(peer_cert_data.to_owned());
            let spki = SubjectPublicKeyInfoDer::from_pem(
                SectionKind::PublicKey,
                peer_cert_data.to_owned(),
            )
            .ok_or(anyhow::anyhow!("failed to parse public key"))?;
            let ver = Arc::new(SimpleRpkClientCertVerifier::new(vec![spki]));
            assert!(ver.requires_raw_public_keys());
            ver
        }
        None | Some(CredentialsType::Any) => {
            anyhow::bail!("client sent unknown cert type {}", peer_cert.tag_raw())
        }
    };
    let builder = rustls::ServerConfig::builder().with_client_cert_verifier(verifier);
    let mut tls_config = match Credentials::type_tag_for(compat, Some(*config.tls_auth_type)) {
        CredentialsType::Any => unreachable!(),
        CredentialsType::X509 => builder.with_single_cert(
            vec![our_creds.certificate().clone()],
            our_creds.private_key_der(),
        )?,
        CredentialsType::RawPublicKey => {
            let resolver =
                AlwaysResolvesServerRawPublicKeys::new(our_creds.as_raw_public_key()?.into());
            builder.with_cert_resolver(Arc::new(resolver))
        }
    };
    tls_config.max_early_data_size = u32::MAX;
    let qsc = QuicServerConfig::try_from(tls_config)?;
    let mut server_cfg = QuinnServerConfig::with_crypto(Arc::new(qsc));
    let _ = server_cfg.transport_config(crate::transport::create_config(config, mode, compat)?.0);
    Ok(server_cfg)
}

fn client_config(
    our_creds: &Credentials,
    peer_cert: &TaggedData<CredentialsType>,
    compat: Compatibility,
    config: &Configuration,
    mode: ThroughputMode,
) -> Result<QuinnClientConfig> {
    let peer_cert_data = peer_cert.data.as_bytes_ref();
    let Some(peer_cert_data) = peer_cert_data else {
        anyhow::bail!("Missing peer credentials");
    };

    let builder = match peer_cert.tag() {
        Some(CredentialsType::X509) => {
            let mut root_store = RootCertStore::empty();
            root_store.add(peer_cert_data.as_slice().into())?;
            let verifier = WebPkiServerVerifier::builder(root_store.into()).build()?;
            rustls::ClientConfig::builder().with_webpki_verifier(verifier)
        }
        Some(CredentialsType::RawPublicKey) => {
            let spki = SubjectPublicKeyInfoDer::from(peer_cert_data.to_owned());
            let ver = Arc::new(SimpleRpkServerCertVerifier::new(vec![spki]));
            assert!(ver.requires_raw_public_keys());
            rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(ver)
        }
        None | Some(CredentialsType::Any) => {
            anyhow::bail!("server sent unknown cert type {}", peer_cert.tag_raw())
        }
    };

    let tls_config = match Credentials::type_tag_for(compat, Some(*config.tls_auth_type)) {
        CredentialsType::Any => unreachable!(),
        CredentialsType::X509 => builder.with_client_auth_cert(
            vec![our_creds.certificate().clone()],
            our_creds.private_key_der(),
        )?,
        CredentialsType::RawPublicKey => {
            let res = AlwaysResolvesClientRawPublicKeys::new(our_creds.as_raw_public_key()?.into());
            builder.with_client_cert_resolver(Arc::new(res))
        }
    };

    let mut client_cfg = QuinnClientConfig::new(Arc::new(QuicClientConfig::try_from(tls_config)?));
    let _ = client_cfg.transport_config(crate::transport::create_config(config, mode, compat)?.0);

    Ok(client_cfg)
}
