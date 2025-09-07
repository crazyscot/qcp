//! Security testing - Man-in-the-Middle (MITM) attacks
// (c) 2025 Ross Younger

use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use rustls_pki_types::CertificateDer;
use tokio::time::timeout;
use x509_certificate::EcdsaCurve;

use qcp::{
    Configuration,
    protocol::{
        compat::Feature,
        control::{Compatibility, ConnectionType},
    },
    transport::ThroughputMode,
    util::Credentials,
};
use qcp::{
    control::create_endpoint,
    protocol::{DataTag, control::CredentialsType},
};

/// This simulates the scenario where either a rogue client attempts to connect to a server,
/// or there is a Man in the Middle attack going on.
///
/// * `modify_certs_fn` (closure type `F`): This closure is called with the original client
///   and server certificates. It is expected to return the same tuple, but may modify or replace
///   either of them. These certificates are passed to the QUIC endpoints as the `peer_cert` parameter.
///
/// * `check_fn` (closure type `G`): This closure is called with the results of the client and server
///   QUIC connection attempts. It is expected to assert that the connections are in the expected state.
async fn run_endpoint_connection<F, G>(
    modify_certs_fn: F,
    check_fn: G,
    compat: Compatibility,
) -> anyhow::Result<()>
where
    F: FnOnce(&Credentials, &Credentials) -> (CertificateDer<'static>, CertificateDer<'static>),
    G: FnOnce(
        anyhow::Result<quinn::Connection>,
        anyhow::Result<quinn::Connection>,
    ) -> anyhow::Result<()>,
{
    let client_credentials = Credentials::generate()?;
    let server_credentials = Credentials::generate()?;
    // CLOSURE 1: Mess with the certificates.
    let (cli_cert_messed, srv_cert_messed) =
        modify_certs_fn(&client_credentials, &server_credentials);

    let cli_cert_messed = if compat.supports(Feature::CMSG_SMSG_2) {
        CredentialsType::RawPublicKey.with_variant(cli_cert_messed.to_vec().into())
    } else {
        CredentialsType::X509.with_variant(cli_cert_messed.to_vec().into())
    };
    let srv_cert_messed = if compat.supports(Feature::CMSG_SMSG_2) {
        CredentialsType::RawPublicKey.with_variant(srv_cert_messed.to_vec().into())
    } else {
        CredentialsType::X509.with_variant(srv_cert_messed.to_vec().into())
    };

    let (server_endpoint, _) = create_endpoint(
        &server_credentials,
        &cli_cert_messed,
        ConnectionType::Ipv4,
        Configuration::system_default(),
        ThroughputMode::Both,
        true,
        compat,
    )?;
    let conn_addr = server_endpoint.local_addr()?;
    eprintln!("Server bound to {conn_addr:?}");
    let conn_addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), conn_addr.port());
    let srv_name = server_credentials.hostname.to_string();

    let (client_endpoint, _) = create_endpoint(
        &client_credentials,
        &srv_cert_messed,
        ConnectionType::Ipv4,
        Configuration::system_default(),
        ThroughputMode::Both,
        false,
        compat,
    )?;
    eprintln!("Client bound to {:?}", client_endpoint.local_addr()?);

    let srv_hdl = tokio::spawn(async move {
        eprintln!("SERVER: accepting");
        let connecting = timeout(Duration::from_secs(5), server_endpoint.accept())
            .await?
            .ok_or(anyhow::anyhow!("server ended"))
            .and_then(|i| Ok(i.accept()?));

        if let Ok(c) = connecting {
            Ok(c.await?)
        } else {
            anyhow::bail!("server accept failed");
        }
        // a successful result is Ok(Connection { ... })
    });
    let cli_hdl = tokio::spawn(async move {
        eprintln!("CLIENT: connecting to {conn_addr:?}");
        timeout(
            Duration::from_secs(5),
            client_endpoint.connect(conn_addr, &srv_name)?,
        )
        .await
        .map_err(|_| anyhow::anyhow!("client connect timed out"))?
        .map_err(|e| anyhow::anyhow!("client connect failed: {e}"))
        // a successful result is Ok(Connection { ... })
    });

    tokio::pin!(srv_hdl, cli_hdl);
    let res = tokio::join!(srv_hdl, cli_hdl);
    let (srv_res, cli_res) = res;
    // simply unwrap the potential join errors as we don't care about those
    let srv_res = srv_res.unwrap();
    let cli_res = cli_res.unwrap();

    // CLOSURE 2: reason about the results
    check_fn(cli_res, srv_res)
}

// X509 ---------------------------------------------------------------------

#[cfg_attr(
    all(target_os = "windows", target_env = "gnu"),
    ignore = "Doesn't work with the mingw cross-compile test runner"
)]
#[tokio::test]
async fn test_x509_ok() {
    // Base case for the scenario. No messing with the certs => all is OK
    run_endpoint_connection(
        |cli, srv| (cli.certificate().to_owned(), srv.certificate().to_owned()),
        |cli_res, srv_res| {
            assert!(cli_res.is_ok());
            assert!(srv_res.is_ok());
            Ok(())
        },
        Compatibility::Level(1),
    )
    .await
    .unwrap();
}

/// Replaces the certificate with a new self-signed one.
///
/// This simulates a Man-in-the-Middle attack in either direction,
/// or an unauthorised client trying to connect to the QCP server endpoint.
fn replace_certificate(der: &CertificateDer<'static>) -> Vec<u8> {
    use x509_certificate::{KeyAlgorithm, X509Certificate, X509CertificateBuilder};
    // It's a self signed cert, we don't need to worry about chains.
    let parsed = X509Certificate::from_der(der).unwrap();
    //eprintln!("PARSED: {parsed:#?}");

    let mut builder = X509CertificateBuilder::default();
    let _ = builder
        .subject()
        .append_common_name_utf8_string(&parsed.subject_common_name().unwrap());
    let (newcert, _keypair) = builder
        .create_with_random_keypair(KeyAlgorithm::Ecdsa(EcdsaCurve::Secp256r1))
        .unwrap();

    //eprintln!("NEWCERT: {newcert:#?}");
    newcert.encode_der().unwrap()
}

#[cfg_attr(
    all(target_os = "windows", target_env = "gnu"),
    ignore = "Doesn't work with the mingw cross-compile test runner"
)]
#[tokio::test]
async fn test_client_x509_mismatch() {
    // mess with the client certificate: client doesn't care, but server refuses it
    run_endpoint_connection(
        |cli, srv| {
            (
                replace_certificate(cli.certificate()).into(),
                srv.certificate().to_owned(),
            )
        },
        |cli_res, srv_res| {
            assert!(cli_res.is_ok());
            assert!(srv_res.is_err());
            let err = srv_res.unwrap_err();
            assert!(err.to_string().contains("invalid peer certificate"));
            eprintln!("Server result: {err}");
            Ok(())
        },
        Compatibility::Level(1),
    )
    .await
    .unwrap();
}

#[cfg_attr(
    all(target_os = "windows", target_env = "gnu"),
    ignore = "Doesn't work with the mingw cross-compile test runner"
)]
#[tokio::test]
async fn test_server_x509_mismatch() {
    // mess with the server certificate: client refuses to connect AND server repors that the client aborted
    run_endpoint_connection(
        |cli, srv| {
            (
                cli.certificate().to_owned(),
                replace_certificate(srv.certificate()).into(),
            )
        },
        |cli_res, srv_res| {
            assert!(cli_res.is_err());
            let err = cli_res.unwrap_err();
            assert!(err.to_string().contains("invalid peer certificate"));
            eprintln!("Client result: {err}");

            assert!(srv_res.is_err());
            let err = srv_res.unwrap_err();
            assert!(err.to_string().contains("invalid peer certificate"));
            eprintln!("Server result: {err}");
            Ok(())
        },
        Compatibility::Level(1),
    )
    .await
    .unwrap();
}

// RPK ---------------------------------------------------------------------

#[cfg_attr(
    all(target_os = "windows", target_env = "gnu"),
    ignore = "Doesn't work with the mingw cross-compile test runner"
)]
#[tokio::test]
async fn test_rpk_ok() {
    // Base case for the scenario. No messing with the certs => all is OK
    run_endpoint_connection(
        |cli, srv| {
            (
                cli.as_raw_public_key().unwrap().cert[0].to_owned(),
                srv.as_raw_public_key().unwrap().cert[0].to_owned(),
            )
        },
        |cli_res, srv_res| {
            assert!(cli_res.inspect_err(|e| eprintln!("{e}")).is_ok());
            assert!(srv_res.inspect_err(|e| eprintln!("{e}")).is_ok());
            Ok(())
        },
        Compatibility::Level(3),
    )
    .await
    .unwrap();
}

/// Replaces the certificate (which is really an RFC7250 Raw Public Key) with a different one.
///
/// This simulates a Man-in-the-Middle attack in either direction,
/// or an unauthorised client trying to connect to the QCP server endpoint.
fn replace_rpk() -> CertificateDer<'static> {
    let creds = Credentials::generate().unwrap();
    let rpk = creds.as_raw_public_key().unwrap();
    rpk.cert[0].clone()
}

#[cfg_attr(
    all(target_os = "windows", target_env = "gnu"),
    ignore = "Doesn't work with the mingw cross-compile test runner"
)]
#[tokio::test]
async fn test_client_rpk_mismatch() {
    // mess with the client certificate: client doesn't care, but server refuses it
    run_endpoint_connection(
        |_cli, srv| {
            (
                replace_rpk().into(),
                srv.as_raw_public_key().unwrap().cert[0].to_owned(),
            )
        },
        |cli_res, srv_res| {
            assert!(cli_res.inspect_err(|e| eprintln!("{e}")).is_ok());
            assert!(srv_res.is_err());
            let err = srv_res.unwrap_err();
            assert!(err.to_string().contains("invalid peer certificate"));
            eprintln!("Server result: {err}");
            Ok(())
        },
        Compatibility::Level(3),
    )
    .await
    .unwrap();
}

#[cfg_attr(
    all(target_os = "windows", target_env = "gnu"),
    ignore = "Doesn't work with the mingw cross-compile test runner"
)]
#[tokio::test]
async fn test_server_rpk_mismatch() {
    // mess with the server certificate: client refuses to connect AND server repors that the client aborted
    run_endpoint_connection(
        |cli, _srv| {
            (
                cli.as_raw_public_key().unwrap().cert[0].to_owned(),
                replace_rpk().into(),
            )
        },
        |cli_res, srv_res| {
            assert!(cli_res.is_err());
            let err = cli_res.unwrap_err();
            assert!(err.to_string().contains("invalid peer certificate"));
            eprintln!("Client result: {err}");

            assert!(srv_res.is_err());
            let err = srv_res.unwrap_err();
            assert!(err.to_string().contains("invalid peer certificate"));
            eprintln!("Server result: {err}");
            Ok(())
        },
        Compatibility::Level(3),
    )
    .await
    .unwrap();
}
