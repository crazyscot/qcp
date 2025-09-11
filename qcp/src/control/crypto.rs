//! Control channel cryptography helpers
// (c) 2025 Ross Younger

use cfg_if::cfg_if;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring::cipher_suite::{
    TLS13_AES_128_GCM_SHA256, TLS13_AES_256_GCM_SHA384, TLS13_CHACHA20_POLY1305_SHA256,
};
use rustls::crypto::{
    WebPkiSupportedAlgorithms, ring as provider, verify_tls13_signature_with_raw_key,
};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    CertificateError, DigitallySignedStruct, DistinguishedName, Error, PeerIncompatible,
    SignatureScheme, SupportedCipherSuite,
};
use rustls_pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime};
use tracing::debug;

/// Verifies the tls handshake signature of the client,
/// and that the client's raw public key is in the list of trusted keys.
///
/// Note: when the verifier is used for Raw Public Keys the `CertificateDer` argument to the functions contains the SPKI instead of a X509 Certificate
///
// Adapted from rustls/openssl-tests/src/raw_key_openssl_interop.rs
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
///
// Adapted from rustls/openssl-tests/src/raw_key_openssl_interop.rs
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

// CIPHER SUITE SELECTION ------------------------------------------------

/// Suite list if the user prefers AES256.
///
/// This suite set does not support ChaCha20.
///
/// Note that QUIC 1.0 forces the use of AES128 for the Initial Packet, so we cannot turn AES128 off completely.
/// This means that should a build of qcp be created which does not support AES256, this suite could degrade to AES128.
/// It is not currently possible to avoid this situation with the current version of the QUIC protocol.
///
/// * TLS13_AES_256_GCM_SHA384
/// * TLS13_AES_128_GCM_SHA256
pub const CIPHER_SUITES_FORCE_AES256: &[SupportedCipherSuite] =
    &[TLS13_AES_256_GCM_SHA384, TLS13_AES_128_GCM_SHA256];

// These sets of cipher suite preferences are those used by golang, see
// https://cs.opensource.google/go/go/+/9d0819b27ca248f9949e7cf6bf7cb9fe7cf574e8:src/crypto/tls/cipher_suites.go;l=342-355

/// The standard cipher suite set, used where the CPU has AES support.
///
/// This allows the other side to express a strong preference for ChaCha20.
///
/// * TLS13_AES_128_GCM_SHA256
/// * TLS13_AES_256_GCM_SHA384
/// * TLS13_CHACHA20_POLY1305_SHA256
pub const CIPHER_SUITES_DEFAULT: &[SupportedCipherSuite] = &[
    TLS13_AES_128_GCM_SHA256,
    TLS13_AES_256_GCM_SHA384,
    TLS13_CHACHA20_POLY1305_SHA256,
];

/// Alternative cipher suite set for situations where the CPU does not have internal AES acceleration support,
/// and we are not configured to force AES256.
///
/// This suite prefers ChaCha20 but will use either AES option if required to.
///
/// * TLS13_CHACHA20_POLY1305_SHA256
/// * TLS13_AES_128_GCM_SHA256
/// * TLS13_AES_256_GCM_SHA384
pub const CIPHER_SUITES_NO_AES_HW: &[SupportedCipherSuite] = &[
    TLS13_CHACHA20_POLY1305_SHA256,
    TLS13_AES_128_GCM_SHA256,
    TLS13_AES_256_GCM_SHA384,
];

/// Cipher suite selection logic
///
/// Both sides apply this function independently to determine their preference list of cipher suites.
///
/// In TLS it is the server that decides which set of preferences wins.
/// See also [`ignore_client_order`].
///
/// Users who have a preference for AES256 should set the `aes256` configuration option or provide it on the command line.
///
/// |                                    | AES256 configured                            | AES256 not configured                        |
/// | ---------------------------------- | -------------------------------------------- | -------------------------------------------- |
/// | **CPU has AES hardware**           | [Force AES256](`CIPHER_SUITES_FORCE_AES256`) | [Default suite](`CIPHER_SUITES_DEFAULT`)     |
/// | **CPU does NOT have AES hardware** | [Force AES256](`CIPHER_SUITES_FORCE_AES256`) | [Prefer ChaCha20](`CIPHER_SUITES_NO_AES_HW`) |
///
pub fn select_cipher_suites(force_aes256: bool) -> &'static [SupportedCipherSuite] {
    if force_aes256 {
        debug!(
            "AES256 cipher suite mode selected: using suites {:?}",
            CIPHER_SUITES_FORCE_AES256
        );
        CIPHER_SUITES_FORCE_AES256
    } else if cpu_supports_aes() {
        debug!(
            "AES hardware support detected, using default TLS1.3 cipher suites {:?}",
            CIPHER_SUITES_DEFAULT
        );
        CIPHER_SUITES_DEFAULT
    } else {
        debug!(
            "No AES support detected on CPU. Selecting alternate TLS1.3 cipher suites {:?}",
            CIPHER_SUITES_NO_AES_HW
        );
        CIPHER_SUITES_NO_AES_HW
    }
}

/// Does the server have a strong preference to use its choice of cipher suite?
///
/// The server has a strong preference if it is configured to force AES256,
/// or if it does not have AES hardware support. Otherwise, it is happy to respect
/// the client's preferences.
///
/// For context, in TLS it is always the server side that determines which cipher suite to use.
/// The [`rustls`](https://docs.rs/rustls/latest/rustls/) implementation allows us to specify
/// which set of preferences to prioritise; this function provides input to that option.
#[must_use]
pub fn ignore_client_order(force_aes256: bool) -> bool {
    force_aes256 || !cpu_supports_aes()
}

/// Runtime CPU feature detection logic
///
/// This only does anything interesting on the `x86_64` and `aarch64` architectures.
/// Other architectures are assumed to not support AES.
///
/// Note that `aarch64` detection is not well tested and may be OS dependent. As a fallback,
/// it assumes no support if it can't tell.
#[must_use]
pub fn cpu_supports_aes() -> bool {
    cfg_if! {
        // These feature detection rules are the same used by golang, see
        // https://cs.opensource.google/go/go/+/9d0819b27ca248f9949e7cf6bf7cb9fe7cf574e8:src/crypto/tls/cipher_suites.go;l=358-359
        if #[cfg(target_arch = "x86_64")] {
            std::arch::is_x86_feature_detected!("aes") && std::arch::is_x86_feature_detected!("pclmulqdq")
        } else if #[cfg(target_arch = "aarch64")] {
            std::arch::is_aarch64_feature_detected!("aes") && std::arch::is_aarch64_feature_detected!("pmull")
        } else {
            debug!("target arch feature autodetection is not supported on {}; assuming no AES support", std::env::consts::ARCH);
            false
        }
    }
}
