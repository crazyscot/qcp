//! Control protocol definitions and helper types
// (c) 2024 Ross Younger
//!
//! The control protocol consists data passed between the local qcp client process and the remote qcp server process
//! before establishing the [QUIC] connection.
//! The two processes are connected via ssh.
//!
//! The control protocol looks like this:
//! * Server ➡️ Client: Banner
//! * C ➡️ S: [`ClientMessage`]
//! * S ➡️ C: [`ServerMessage`]
//! * Client establishes a QUIC connection to the server, on the port given in the [`ServerMessage`].
//! * Client then opens one or more bidirectional QUIC streams ('sessions') on that connection.
//!    (See the session protocol for what happens there.)
//!
//! When transfer is complete and all QUIC streams are closed:
//! * S ➡️ C: [`ClosedownReport`]
//! * C ➡️ S: (closes control channel; server takes this as a cue to exit)
//!
//! On the wire these are [CapnProto] messages, sent using standard framing.
//!
//! [quic]: https://quicwg.github.io/
//! [capnproto]: https://capnproto.org/

pub use super::control_capnp::client_message::ConnectionType;

use super::control_capnp;
use anyhow::Result;
use capnp::message::ReaderOptions;
use quinn::ConnectionStats;
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

/// Server banner message, sent on stdout and checked by the client
pub const BANNER: &str = "qcp-server-1\n";

/// Helper type for [`control_capnp::client_message`]
#[derive(Debug)]
#[allow(missing_docs)]
pub struct ClientMessage {
    pub cert: Vec<u8>,
    pub connection_type: ConnectionType,
}

impl ClientMessage {
    // This is weirdly asymmetric to avoid needless allocs.
    /// One-stop serializer
    pub async fn write<W>(write: &mut W, cert: &[u8], conn_type: ConnectionType) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut builder = msg.init_root::<control_capnp::client_message::Builder<'_>>();
        builder.set_cert(cert);
        builder.set_connection_type(conn_type);
        capnp_futures::serialize::write_message(write.compat_write(), &msg).await?;
        Ok(())
    }
    /// Deserializer
    pub async fn read<R>(read: &mut R) -> Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: control_capnp::client_message::Reader<'_> = reader.get_root()?;
        let cert = msg_reader.get_cert()?.to_vec();
        let connection_type: ConnectionType = msg_reader
            .get_connection_type()
            .map_err(|_| anyhow::anyhow!("incompatible ClientMessage"))?;
        Ok(Self {
            cert,
            connection_type,
        })
    }
}

/// Helper type for [`control_capnp::server_message`]
pub struct ServerMessage {
    /// Port the server is bound to
    pub port: u16,
    /// Certificate data (DER encoded)
    pub cert: Vec<u8>,
    /// Server's idea of its hostname (should match the certificate)
    pub name: String,
    /// Server warning message (if any)
    pub warning: Option<String>,
    /// Server bandwidth information message
    pub bandwidth_info: String,
}

impl std::fmt::Debug for ServerMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerMessage")
            .field("port", &self.port)
            .field("cert length", &self.cert.len())
            .field("name", &self.name)
            .field("warning", &self.warning)
            .field("bandwidth_info", &self.bandwidth_info)
            .finish()
    }
}

impl ServerMessage {
    /// Serializer
    // This is weirdly asymmetric to avoid needless allocs.
    pub async fn write<W>(
        write: &mut W,
        port: u16,
        cert: &[u8],
        name: &str,
        warning: Option<&str>,
        bandwidth_info: &str,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut builder = msg.init_root::<control_capnp::server_message::Builder<'_>>();
        builder.set_port(port);
        builder.set_cert(cert);
        builder.set_name(name);
        if let Some(w) = warning {
            builder.set_warning(w);
        }
        builder.set_bandwidth_info(bandwidth_info);
        capnp_futures::serialize::write_message(write.compat_write(), &msg).await?;
        Ok(())
    }

    /// Deserializer
    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: control_capnp::server_message::Reader<'_> = reader.get_root()?;
        let cert = msg_reader.get_cert()?.to_vec();
        let name = msg_reader.get_name()?.to_str()?.to_string();
        let port = msg_reader.get_port();
        let warning = msg_reader.get_warning()?.to_str()?;
        let warning = if warning.is_empty() {
            None
        } else {
            Some(warning.to_string())
        };
        let bandwidth_info = msg_reader.get_bandwidth_info()?.to_str()?.to_string();
        Ok(Self {
            port,
            cert,
            name,
            warning,
            bandwidth_info,
        })
    }
}

/// Helper type for [`control_capnp::closedown_report`]
#[derive(Clone, Copy, Debug)]
pub struct ClosedownReport {
    /// Final congestion window
    pub cwnd: u64,
    /// Sent packet count
    pub sent_packets: u64,
    /// Send byte count
    pub sent_bytes: u64,
    /// Lost packet count
    pub lost_packets: u64,
    /// Lost packet total payload
    pub lost_bytes: u64,
    /// Number of congestion events detected
    pub congestion_events: u64,
    /// Number of black hole events detected
    pub black_holes_detected: u64,
}

impl ClosedownReport {
    /// Serializer
    pub async fn write<W>(write: &mut W, stats: &ConnectionStats) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let ps = &stats.path;
        let mut msg = ::capnp::message::Builder::new_default();
        let mut builder = msg.init_root::<control_capnp::closedown_report::Builder<'_>>();
        builder.set_final_congestion_window(ps.cwnd);
        builder.set_sent_packets(ps.sent_packets);
        builder.set_sent_bytes(stats.udp_tx.bytes);
        builder.set_lost_packets(ps.lost_packets);
        builder.set_lost_bytes(ps.lost_bytes);
        builder.set_congestion_events(ps.congestion_events);
        builder.set_black_holes(ps.black_holes_detected);
        capnp_futures::serialize::write_message(write.compat_write(), &msg).await?;
        Ok(())
    }

    /// Deserializer
    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: control_capnp::closedown_report::Reader<'_> = reader.get_root()?;
        let cwnd = msg_reader.get_final_congestion_window();
        let sent_packets = msg_reader.get_sent_packets();
        let sent_bytes = msg_reader.get_sent_bytes();
        let lost_packets = msg_reader.get_lost_packets();
        let lost_bytes = msg_reader.get_lost_bytes();
        let congestion_events = msg_reader.get_congestion_events();
        let black_holes_detected = msg_reader.get_black_holes();

        Ok(Self {
            cwnd,
            sent_packets,
            sent_bytes,
            lost_packets,
            lost_bytes,
            congestion_events,
            black_holes_detected,
        })
    }
}

#[cfg(test)]
mod tests {

    // These tests are really only exercising capnp, proving that we know how to drive it correctly.

    use super::{control_capnp, ClientMessage, ServerMessage};
    use anyhow::Result;
    use capnp::{message::ReaderOptions, serialize};

    fn encode_client(cert: &[u8]) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut client_msg = msg.init_root::<control_capnp::client_message::Builder<'_>>();
        client_msg.set_cert(cert);
        serialize::write_message_to_words(&msg)
    }

    fn decode_client(wire: &[u8]) -> Result<ClientMessage> {
        use control_capnp::client_message::{self};
        let reader = serialize::read_message(wire, ReaderOptions::new())?;
        let cert_reader: client_message::Reader<'_> = reader.get_root()?;
        Ok(ClientMessage {
            cert: Vec::<u8>::from(cert_reader.get_cert()?),
            connection_type: cert_reader.get_connection_type()?,
        })
    }
    fn encode_server(port: u16, cert: &[u8]) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut server_msg = msg.init_root::<control_capnp::server_message::Builder<'_>>();
        server_msg.set_port(port);
        server_msg.set_cert(cert);
        serialize::write_message_to_words(&msg)
    }
    fn decode_server(wire: &[u8]) -> Result<ServerMessage> {
        use control_capnp::server_message;
        let reader = serialize::read_message(wire, ReaderOptions::new())?;
        let msg_reader: server_message::Reader<'_> = reader.get_root()?;
        let cert = Vec::<u8>::from(msg_reader.get_cert()?);
        let port = msg_reader.get_port();
        Ok(ServerMessage {
            port,
            cert,
            name: "localhost".to_string(),
            warning: Some("foo".to_string()),
            bandwidth_info: "bar".into(),
        })
    }

    #[test]
    fn client_pairwise_alloc() -> Result<()> {
        // A single round trip test: encode, decode, check.

        // Random certificate data of a given length
        let cert = {
            let mut temp = Vec::<u8>::with_capacity(128);
            temp.fill_with(|| fastrand::u8(0..255));
            temp
        };

        let wire = encode_client(&cert);
        println!("Client message encoded size is {}", wire.len());
        let decoded = decode_client(&wire)?;
        assert_eq!(cert, decoded.cert);
        Ok(())
    }

    #[test]
    fn server_pairwise_alloc() -> Result<()> {
        // A single round trip test: encode, decode, check.

        // Random certificate data of a given length
        let cert = {
            let mut temp = Vec::<u8>::with_capacity(128);
            temp.fill_with(|| fastrand::u8(0..255));
            temp
        };
        let port = fastrand::u16(1..65535);

        let wire = encode_server(port, &cert);
        println!("Server message encoded size is {}", wire.len());
        let decoded = decode_server(&wire)?;
        assert_eq!(cert, decoded.cert);
        assert_eq!(port, decoded.port);
        Ok(())
    }
}
