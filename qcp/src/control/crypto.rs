//! Control channel cryptography helpers
// Adapted from rustls/openssl-tests/src/raw_key_openssl_interop.rs

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{
    WebPkiSupportedAlgorithms, ring as provider, verify_tls13_signature_with_raw_key,
};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    CertificateError, DigitallySignedStruct, DistinguishedName, Error, PeerIncompatible,
    SignatureScheme,
};
use rustls_pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime};

/// Verifies the tls handshake signature of the client,
/// and that the client's raw public key is in the list of trusted keys.
///
/// Note: when the verifier is used for Raw Public Keys the `CertificateDer` argument to the functions contains the SPKI instead of a X509 Certificate
#[derive(Debug)]
pub(crate) struct SimpleRpkClientCertVerifier {
    trusted_spki: Vec<SubjectPublicKeyInfoDer<'static>>,
    supported_algs: WebPkiSupportedAlgorithms,
}

impl SimpleRpkClientCertVerifier {
    pub(crate) fn new(trusted_spki: Vec<SubjectPublicKeyInfoDer<'static>>) -> Self {
        Self {
            trusted_spki,
            supported_algs: provider::default_provider().signature_verification_algorithms,
        }
    }
}

impl ClientCertVerifier for SimpleRpkClientCertVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        let end_entity_as_spki = SubjectPublicKeyInfoDer::from(end_entity.as_ref());
        if self.trusted_spki.contains(&end_entity_as_spki) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(Error::InvalidCertificate(CertificateError::UnknownIssuer))
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::PeerIncompatible(PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
        //input: &SignatureVerificationInput<'_>,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature_with_raw_key(
            message,
            &SubjectPublicKeyInfoDer::from(cert.as_ref()),
            dss,
            &self.supported_algs,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_algs.supported_schemes()
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}

/// Verifies the tls handshake signature of the server,
/// and that the server's raw public key is in the list of trusted keys.
///
/// Note: when the verifier is used for Raw Public Keys the `CertificateDer` argument to the functions contains the SPKI instead of a X509 Certificate
#[derive(Debug)]
pub(crate) struct SimpleRpkServerCertVerifier {
    trusted_spki: Vec<SubjectPublicKeyInfoDer<'static>>,
    supported_algs: WebPkiSupportedAlgorithms,
}

impl SimpleRpkServerCertVerifier {
    pub(crate) fn new(trusted_spki: Vec<SubjectPublicKeyInfoDer<'static>>) -> Self {
        Self {
            trusted_spki,
            supported_algs: provider::default_provider().signature_verification_algorithms,
        }
    }
}

impl ServerCertVerifier for SimpleRpkServerCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let end_entity_as_spki = SubjectPublicKeyInfoDer::from(end_entity.as_ref());
        if self.trusted_spki.contains(&end_entity_as_spki) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::InvalidCertificate(CertificateError::UnknownIssuer))
        }
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::PeerIncompatible(PeerIncompatible::Tls12NotOffered))
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        verify_tls13_signature_with_raw_key(
            message,
            &SubjectPublicKeyInfoDer::from(cert.as_ref()),
            dss,
            &self.supported_algs,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.supported_algs.supported_schemes()
    }

    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}
