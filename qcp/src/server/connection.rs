//! Handler for an incoming connection as a whole
// (c) 2024 Ross Younger

use super::handle_stream;
use crate::protocol::common::SendReceivePair;

use quinn::ConnectionStats;
use tracing::{debug, error, trace};

pub(super) async fn handle_connection(conn: quinn::Incoming) -> anyhow::Result<ConnectionStats> {
    let connection = conn.await?;
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
            let _j = tokio::spawn(async move {
                if let Err(e) = handle_stream(sp).await {
                    error!("stream handler failed: {e}");
                }
            });
        }
    }
    .await?;
    Ok(connection.stats())
}
