//! X509 certificate management helper
// (c) 2024 Ross Younger

use std::{borrow::Borrow, sync::Arc};

use anyhow::Result;
use quinn::rustls::sign::{CertifiedKey as RustlsCertifiedKey, SigningKey as RustlsSigningKey};
use rcgen::{CertifiedKey as RcgenCertifiedKey, KeyPair as RcgenKeyPair, PublicKeyData};
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};

use crate::protocol::{
    DataTag as _, TaggedData,
    compat::Feature,
    control::{Compatibility, CredentialsType},
};

/// In-memory representation of TLS credentials
#[allow(missing_debug_implementations)]
pub struct Credentials {
    /// A keypair with self-signed X509 certificate
    pub keypair: RcgenCertifiedKey<RcgenKeyPair>,
    /// The hostname to which this applies (for convenience)
    pub hostname: String,
}

impl Credentials {
    /// Generates a self-certified keypair
    pub fn generate() -> Result<Self> {
        let hostname = gethostname::gethostname()
            .into_string()
            .unwrap_or("unknown.host.invalid".to_string());
        tracing::trace!("Creating certificate with hostname {hostname}");
        Ok(Self {
            keypair: rcgen::generate_simple_self_signed([hostname.clone()])?,
            hostname,
        })
    }
    /// Extracts the certificate in DER format
    #[must_use]
    pub fn certificate(&self) -> &CertificateDer<'static> {
        self.keypair.cert.der()
    }

    /// Extracts the private key in DER format
    #[must_use]
    pub fn private_key_der(&self) -> PrivateKeyDer<'static> {
        rustls_pki_types::PrivateKeyDer::Pkcs8(self.keypair.signing_key.serialize_der().into())
    }

    /// Returns the raw public key as a `CertifiedKey` in RFC7250 format
    /// (i.e., with the SPKI inside a `CertificateDer`),
    /// suitable for use with `AlwaysResolvesClientRawPublicKeys` etc.
    pub fn as_raw_public_key(&self) -> Result<RustlsCertifiedKey> {
        let spki = self.keypair.signing_key.subject_public_key_info();
        let public_key_cert = CertificateDer::from(spki);
        let kp_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
            self.keypair.signing_key.serialized_der(),
        ));
        let signing_key: Arc<dyn RustlsSigningKey> =
            rustls::crypto::ring::sign::any_supported_type(&kp_der)?;

        Ok(RustlsCertifiedKey::new(vec![public_key_cert], signing_key))
    }

    /// Determines the credentials type to use for the control channel,
    /// based on the chosen compatibility mode.
    /// * If the compatibility mode supports it, uses RFC7250 raw public keys.
    /// * Otherwise, uses the self-signed X509 certificate.
    #[must_use]
    pub fn type_tag_for(
        compat: Compatibility,
        configured_type: Option<CredentialsType>,
    ) -> CredentialsType {
        match configured_type {
            // in a config struct, 'invalid' means unset i.e. default to automatic
            None | Some(CredentialsType::Any) => (),
            Some(other) => return other,
        }
        if compat.supports(Feature::CMSG_SMSG_2) {
            tracing::trace!("selected creds type: Rfc7250");
            CredentialsType::RawPublicKey
        } else {
            tracing::trace!("selected creds type: X509");
            CredentialsType::X509
        }
    }

    /// Converts to a TaggedData suitable for sending over the control channel
    /// * If the compatibility mode supports it, uses RFC7250 raw public keys.
    /// * Otherwise, uses the self-signed X509 certificate.
    pub fn to_tagged_data(
        &self,
        compat: Compatibility,
        configured_type: Option<CredentialsType>,
    ) -> Result<TaggedData<CredentialsType>> {
        let tag = Self::type_tag_for(compat, configured_type);
        let res = match tag {
            CredentialsType::Any => unreachable!(),
            CredentialsType::X509 => {
                // Compat level 1 supports this
                let cert_bytes: &[u8] = self.certificate();
                tag.with_variant(cert_bytes.into())
            }
            CredentialsType::RawPublicKey => {
                // Compat level 3 needed to support this
                anyhow::ensure!(
                    compat.supports(Feature::CMSG_SMSG_2),
                    "RawPublicKey credentials configured, but not supported by remote",
                );
                let key = self.as_raw_public_key()?;
                let borrowed: &rustls::sign::CertifiedKey = key.borrow();
                let cert: &[u8] = &borrowed.cert[0];
                tag.with_variant(cert.into())
            }
        };
        Ok(res)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use pretty_assertions::assert_eq;

    use crate::{
        protocol::control::{Compatibility, CredentialsType},
        util::Credentials,
    };
    #[test]
    fn generate_works() {
        let _ = super::Credentials::generate().unwrap();
    }

    #[test]
    fn type_tag_cases() {
        assert_eq!(
            Credentials::type_tag_for(Compatibility::Level(3), Some(CredentialsType::RawPublicKey)),
            CredentialsType::RawPublicKey
        );
        assert_eq!(
            Credentials::type_tag_for(Compatibility::Level(3), Some(CredentialsType::X509)),
            CredentialsType::X509
        );

        let c = Credentials::generate().unwrap();
        let e = c
            .to_tagged_data(Compatibility::Level(1), Some(CredentialsType::RawPublicKey))
            .unwrap_err();
        assert_eq!(
            e.to_string(),
            "RawPublicKey credentials configured, but not supported by remote"
        );
    }
}
