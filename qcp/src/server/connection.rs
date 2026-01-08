//! Handler for an incoming connection as a whole
// (c) 2024 Ross Younger

use super::handle_stream;
use crate::{
    Configuration,
    protocol::{
        common::{ReceivingStream as QcpRS, SendReceivePair, SendingStream as QcpSS},
        control::Compatibility,
    },
};

use async_trait::async_trait;
use quinn::ConnectionStats;
use tracing::{debug, error, trace};

#[async_trait]
trait Connection<SS: QcpSS, RS: QcpRS> {
    async fn accept_bi(&self) -> Result<(SS, RS), quinn::ConnectionError>;
    fn stats(&self) -> ConnectionStats;
    fn remote_address(&self) -> std::net::SocketAddr;
}

#[cfg_attr(coverage_nightly, coverage(off))] // This is a thin adaptor, not worth testing
#[async_trait]
impl Connection<quinn::SendStream, quinn::RecvStream> for quinn::Connection {
    async fn accept_bi(
        &self,
    ) -> Result<(quinn::SendStream, quinn::RecvStream), quinn::ConnectionError> {
        self.accept_bi().await
    }

    fn stats(&self) -> ConnectionStats {
        self.stats()
    }

    fn remote_address(&self) -> std::net::SocketAddr {
        self.remote_address()
    }
}

#[cfg_attr(coverage_nightly, coverage(off))] // This is a thin adaptor, not worth testing
pub(super) async fn handle_incoming(
    i: quinn::Incoming,
    compat: Compatibility,
    config: &Configuration,
) -> anyhow::Result<ConnectionStats> {
    handle_inner(i.await?, compat, config).await
}

async fn handle_inner<SS: QcpSS + 'static, RS: QcpRS + 'static, C: Connection<SS, RS>>(
    connection: C,
    compat: Compatibility,
    config: &Configuration,
) -> anyhow::Result<ConnectionStats> {
    debug!(
        "accepted QUIC connection from {}",
        connection.remote_address()
    );

    async {
        loop {
            let stream = connection.accept_bi().await;
            let sp = match stream {
                Err(quinn::ConnectionError::ApplicationClosed { .. }) => {
                    // we're closing down
                    debug!("application closing");
                    return Ok::<(), anyhow::Error>(());
                }
                Err(quinn::ConnectionError::ConnectionClosed { .. }) => {
                    debug!("connection closed by remote");
                    return Ok::<(), anyhow::Error>(());
                }
                Err(e) => {
                    error!("connection error: {e}");
                    return Err(e.into());
                }
                Ok(s) => SendReceivePair::from(s),
            };
            trace!("opened stream");
            let cfg = config.clone();
            let _j = tokio::spawn(async move {
                if let Err(e) = handle_stream(sp, compat, &cfg).await {
                    error!("stream handler failed: {e}");
                }
            });
        }
    }
    .await?;
    Ok(connection.stats())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::net::{Ipv4Addr, SocketAddrV4};

    use crate::Configuration;
    use crate::{protocol::control::Compatibility, server::connection::handle_inner};

    use super::Connection;

    use assertables::assert_contains;
    use async_trait::async_trait;
    use quinn::{ApplicationClose, ConnectionClose, ConnectionStats, TransportErrorCode};
    use tokio_test::io::{Builder, Mock};

    #[derive(Default)]
    struct MockConnection {
        err: Option<quinn::ConnectionError>,
        ok_count: tokio::sync::Mutex<u32>,
    }

    impl MockConnection {
        fn err(e: quinn::ConnectionError) -> Self {
            Self {
                err: Some(e),
                ..Default::default()
            }
        }
    }

    #[async_trait]
    impl Connection<Mock, Mock> for MockConnection {
        async fn accept_bi(&self) -> Result<(Mock, Mock), quinn::ConnectionError> {
            if let Some(e) = &self.err {
                return Err(e.clone());
            }
            let mut m = self.ok_count.lock().await;
            if *m == 0 {
                return Err(quinn::ConnectionError::ApplicationClosed(
                    ApplicationClose {
                        error_code: 0u8.into(),
                        reason: "done".into(),
                    },
                ));
            }
            *m -= 1;
            let buf = &[1u8, 0, 0, 0]; // the inner will fail to read the nonexistent following data, but we don't care
            let mock_recv = Builder::new().read(buf).build();
            let mock_send = Builder::new().build();
            Ok((mock_send, mock_recv))
        }

        fn stats(&self) -> ConnectionStats {
            ConnectionStats::default()
        }

        fn remote_address(&self) -> std::net::SocketAddr {
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 8765).into()
        }
    }

    #[tokio::test]
    async fn timeout() {
        let mc = MockConnection::err(quinn::ConnectionError::TimedOut);
        let e = handle_inner(mc, Compatibility::Level(1), Configuration::system_default())
            .await
            .unwrap_err();
        assert_contains!(e.to_string(), "timed out");
    }
    #[tokio::test]
    async fn conn_closed() {
        let mc = MockConnection::err(quinn::ConnectionError::ConnectionClosed(ConnectionClose {
            error_code: TransportErrorCode::crypto(1),
            frame_type: None,
            reason: "no".into(),
        }));
        let s = handle_inner(mc, Compatibility::Level(1), Configuration::system_default())
            .await
            .unwrap();
        assert_eq!(s.path.sent_packets, 0);
    }

    #[tokio::test]
    async fn conn_ok() {
        let mc = MockConnection {
            ok_count: 1.into(),
            ..Default::default()
        };
        let s = handle_inner(mc, Compatibility::Level(1), Configuration::system_default())
            .await
            .unwrap();
        assert_eq!(s.path.sent_packets, 0);
    }
}
