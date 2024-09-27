// QCP control protocol
// (c) 2024 Ross Younger

/*
 * The control protocol is data passed between the local qcp client process and the remote qcp server process
 * before establishing the QUIC connection.
 * The two processes are usually connected by stdio, via ssh.
 *
 * The protocol looks like this:
 *   (Client creates remote server process)
 *   Server -> Client: Banner
 *   C -> S: `ClientMessage`
 *   S -> C: `ServerMessage`
 * The client then establishes a QUIC connection to the server, on the port given in the `ServerMessage`.
 * The client then opens one or more bidirectional QUIC streams ('sessions') on that connection.
 * See the session protocol for what happens there.
 *
 * On the wire the Client and Server messages are sent using capnproto with standard framing.
 */

use crate::util::AddressFamily;

use anyhow::Result;
use capnp::message::ReaderOptions;
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

pub mod control_capnp {
    include!(concat!(env!("OUT_DIR"), "/control_capnp.rs"));
}

pub const BANNER: &str = "qcpcs\n";

/// Rust type analogue to the capnproto struct
#[derive(Debug)]
pub struct ClientMessage {
    pub cert: Vec<u8>,
    pub connection_type: AddressFamily,
}

impl ClientMessage {
    // This is weirdly asymmetric to avoid needless allocs.
    pub async fn write<W>(write: &mut W, cert: &[u8], conn_type: AddressFamily) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut builder = msg.init_root::<control_capnp::client_message::Builder>();
        builder.set_cert(cert);
        builder.set_connection_type(conn_type.try_into()?);
        capnp_futures::serialize::write_message(write.compat_write(), &msg).await?;
        Ok(())
    }
    pub async fn read<R>(read: &mut R) -> Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        use control_capnp::client_message::ConnectionType as wire_af;

        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: control_capnp::client_message::Reader = reader.get_root()?;
        let cert = msg_reader.get_cert()?.to_vec();
        let conn_type: wire_af = msg_reader.get_connection_type()?;
        Ok(Self {
            cert,
            connection_type: conn_type.into(),
        })
    }
}

/// Rust type analogue to the capnproto struct
#[derive(Debug)]
pub struct ServerMessage {
    /// Port the server is bound to
    pub port: u16,
    /// Certificate data (DER encoded)
    pub cert: Vec<u8>,
    /// Server's idea of its hostname (should match the certificate)
    pub name: String,
    /// Server warning message (if any)
    pub warning: Option<String>,
}

impl ServerMessage {
    // This is weirdly asymmetric to avoid needless allocs.
    pub async fn write<W>(
        write: &mut W,
        port: u16,
        cert: &[u8],
        name: &str,
        warning: Option<&str>,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut builder = msg.init_root::<control_capnp::server_message::Builder>();
        builder.set_port(port);
        builder.set_cert(cert);
        builder.set_name(name);
        if let Some(w) = warning {
            builder.set_warning(w);
        }
        capnp_futures::serialize::write_message(write.compat_write(), &msg).await?;
        Ok(())
    }

    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: control_capnp::server_message::Reader = reader.get_root()?;
        let cert = msg_reader.get_cert()?.to_vec();
        let name = msg_reader.get_name()?.to_str()?.to_string();
        let port = msg_reader.get_port();
        let warning = msg_reader.get_warning()?.to_str()?.to_string();
        let warning = if warning.is_empty() {
            None
        } else {
            Some(warning)
        };
        Ok(Self {
            port,
            cert,
            name,
            warning,
        })
    }
}

#[cfg(test)]
mod tests {

    // These tests are really only exercising capnp, proving that we know how to drive it correctly.

    use crate::util::AddressFamily;

    use super::{control_capnp, ClientMessage, ServerMessage};
    use anyhow::Result;
    use capnp::{message::ReaderOptions, serialize};

    pub fn encode_client(cert: &[u8]) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut client_msg = msg.init_root::<control_capnp::client_message::Builder>();
        client_msg.set_cert(cert);
        serialize::write_message_to_words(&msg)
    }

    pub fn decode_client(wire: &[u8]) -> Result<ClientMessage> {
        use control_capnp::client_message::{self};
        let reader = serialize::read_message(wire, ReaderOptions::new())?;
        let cert_reader: client_message::Reader = reader.get_root()?;
        let cert = Vec::<u8>::from(cert_reader.get_cert()?);
        let family: AddressFamily = cert_reader.get_connection_type()?.into();
        Ok(ClientMessage {
            cert,
            connection_type: family,
        })
    }
    pub fn encode_server(port: u16, cert: &[u8]) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();
        let mut server_msg = msg.init_root::<control_capnp::server_message::Builder>();
        server_msg.set_port(port);
        server_msg.set_cert(cert);
        serialize::write_message_to_words(&msg)
    }
    pub fn decode_server(wire: &[u8]) -> Result<ServerMessage> {
        use control_capnp::server_message;
        let reader = serialize::read_message(wire, ReaderOptions::new())?;
        let msg_reader: server_message::Reader = reader.get_root()?;
        let cert = Vec::<u8>::from(msg_reader.get_cert()?);
        let port = msg_reader.get_port();
        Ok(ServerMessage {
            port,
            cert,
            name: "localhost".to_string(),
            warning: Some("foo".to_string()),
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
