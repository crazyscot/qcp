//! X509 certificate management helper
// (c) 2024 Ross Younger

use anyhow::Result;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

/// In-memory representation of X509 credentials (for TLS)
#[derive(Debug)]
pub struct Credentials {
    /// X509 certificate
    pub certificate: CertificateDer<'static>,
    /// Keypair the certificate relates to
    pub keypair: PrivateKeyDer<'static>,
    /// Hostname the certificate relates to (convenience member)
    pub hostname: String,
}

/*
fn dump(creds: &rcgen::CertifiedKey) {
    println!("{}{}\n", creds.cert.pem(), creds.key_pair.serialize_pem());
}
*/

impl Credentials {
    /// Factory method
    pub fn generate() -> Result<Self> {
        let hostname = gethostname::gethostname()
            .into_string()
            .unwrap_or("unknown.host.invalid".to_string());
        tracing::trace!("Creating certificate with hostname {hostname}");
        let raw = rcgen::generate_simple_self_signed([hostname.clone()])?;
        Ok(Credentials {
            certificate: raw.cert.der().clone(),
            keypair: rustls_pki_types::PrivateKeyDer::Pkcs8(raw.signing_key.serialize_der().into()),
            hostname,
        })
    }

    /// Cloning accessor
    #[must_use]
    pub(crate) fn cert_chain(&self) -> Vec<CertificateDer<'static>> {
        vec![self.certificate.clone()]
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    #[test]
    fn generate_works() {
        let _ = super::Credentials::generate().unwrap();
    }
}
