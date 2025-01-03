//! QCP session protocol definitions and helper types
// (c) 2024 Ross Younger
//!
//! The session protocol operates over a QUIC bidirectional stream.
//!
//! The protocol consists of [Command] and [Response] packets and helper structs.
//! Packets are sent using the standard CapnProto framing.
//!
//! * Client ➡️ Server: (initiates QUIC stream)
//! * C ➡️ S : [Command] packet. This is an enum containing arguments needed by the selected command.
//! * S ➡️ C : [Response] packet
//! * Then they do whatever is appropriate for the command.
//!
//! The following commands are defined:
//! ### Get
//!
//! Retrieves a file from the remote.
//! * C ➡️ S: [GetArgs] _(within [Command])_
//! * S ➡️ C: [Response] . If the status within was not OK, the command does not proceed.
//! * S ➡️ C: [FileHeader], file data, [FileTrailer].
//!
//! After transfer, close the stream.
//!
//! Either side may close the stream mid-flow if it needs to abort the transfer.
//!
//! ### Put
//!
//! Sends a file to the remote.
//! * C ➡️ S: [PutArgs] _(within [Command])_
//! * S ➡️ C: [Response] to the command
//! * C ➡️ S: [FileHeader], file data, [FileTrailer].
//! * S ➡️ C: [Response] indicating transfer status
//!
//! After transfer, close the stream.
//!
//! If the server needs to abort the transfer mid-flow, it may send a Response explaining why, then close the stream.
//!
//! [quic]: https://quicwg.github.io/
//! [capnproto]: https://capnproto.org/

pub use super::session_capnp::Status;

use super::session_capnp;
use anyhow::Result;
use capnp::message::ReaderOptions;
use std::fmt::Display;
use tokio_util::compat::TokioAsyncReadCompatExt as _;

/// Command packet
#[derive(Debug, strum::Display)]
#[allow(missing_docs)]
pub enum Command {
    Get(GetArgs),
    Put(PutArgs),
}
#[derive(Debug)]
/// Arguments for [Command::Get]
#[allow(missing_docs)]
pub struct GetArgs {
    pub filename: String,
}
#[derive(Debug)]
/// Arguments for [Command::Put]
#[allow(missing_docs)]
pub struct PutArgs {
    pub filename: String,
}

impl Command {
    /// Specialised constructor for Get
    #[must_use]
    pub fn new_get(filename: &str) -> Self {
        Self::Get(GetArgs {
            filename: filename.to_string(),
        })
    }
    /// Specialised constructor for Put
    #[must_use]
    pub fn new_put(filename: &str) -> Self {
        Self::Put(PutArgs {
            filename: filename.to_string(),
        })
    }

    /// One-stop serializer
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        use crate::protocol::session::Command::{Get, Put};
        let mut msg = ::capnp::message::Builder::new_default();
        let builder = msg.init_root::<session_capnp::command::Builder<'_>>();
        match self {
            Get(args) => {
                let mut build_args = builder.init_args().init_get();
                build_args.set_filename(&args.filename);
            }
            Put(args) => {
                let mut build_args = builder.init_args().init_put();
                build_args.set_filename(&args.filename);
            }
        }
        capnp::serialize::write_message_to_words(&msg)
    }

    /// Deserializer
    pub async fn read<R>(read: &mut R) -> Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        use session_capnp::command::{
            self,
            args::{Get, Put},
        };
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg: command::Reader<'_> = reader.get_root()?;

        Ok(match msg.get_args().which() {
            Ok(Get(get)) => Command::Get(GetArgs {
                filename: get?.get_filename()?.to_string()?,
            }),
            Ok(Put(put)) => Command::Put(PutArgs {
                filename: put?.get_filename()?.to_string()?,
            }),
            Err(e) => {
                anyhow::bail!("unrecognised command id {}", e.0);
            }
        })
    }
}

#[derive(Debug)]
/// Response packet
#[allow(missing_docs)]
pub struct Response {
    pub status: Status,
    pub message: Option<String>,
}

impl Response {
    /// Serializer
    #[must_use]
    pub fn serialize(&self) -> Vec<u8> {
        Self::serialize_direct(self.status, self.message.as_deref())
    }
    /// Serializer without an intervening object
    #[must_use]
    pub fn serialize_direct(status: Status, message: Option<&str>) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();

        let mut response_msg = msg.init_root::<session_capnp::response::Builder<'_>>();
        response_msg.set_status(status);
        if let Some(s) = message {
            response_msg.set_message(s);
        }
        capnp::serialize::write_message_to_words(&msg)
    }
    /// Deserializer
    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: session_capnp::response::Reader<'_> = reader.get_root()?;
        let status = msg_reader
            .get_status()
            .map_err(|_| anyhow::anyhow!("incompatible Response (missing Status)"))?;
        let message = if msg_reader.has_message() {
            Some(msg_reader.get_message()?.to_string()?)
        } else {
            None
        };
        Ok(Self { status, message })
    }
}

impl Display for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.message {
            Some(msg) => write!(f, "{:?} with message {}", self.status, msg),
            None => write!(f, "{:?}", self.status),
        }
    }
}

#[derive(Debug)]
#[allow(missing_docs)]
/// File Header packet
pub struct FileHeader {
    pub size: u64,
    pub filename: String,
}

impl FileHeader {
    /// One-stop serializer
    #[must_use]
    pub fn serialize_direct(size: u64, filename: &str) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();

        let mut response_msg = msg.init_root::<session_capnp::file_header::Builder<'_>>();
        response_msg.set_size(size);
        response_msg.set_filename(filename);
        capnp::serialize::write_message_to_words(&msg)
    }
    /// Deserializer
    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: session_capnp::file_header::Reader<'_> = reader.get_root()?;
        Ok(Self {
            size: msg_reader.get_size(),
            filename: msg_reader.get_filename()?.to_string()?,
        })
    }
}

#[derive(Debug, Copy, Clone)]
/// File Trailer packet
pub struct FileTrailer {}

impl FileTrailer {
    /// One-stop serializer
    #[must_use]
    pub fn serialize_direct() -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();

        let mut _response_msg = msg.init_root::<session_capnp::file_trailer::Builder<'_>>();
        capnp::serialize::write_message_to_words(&msg)
    }
    /// Deserializer
    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let _msg_reader: session_capnp::file_trailer::Reader<'_> = reader.get_root()?;
        Ok(Self {})
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, FileHeader, FileTrailer, Response, Status};
    #[test]
    fn marshal_size() {
        // not really a test - just a sanity check that nothing has broken
        let c = Command::new_get("filename").serialize();
        println!("Command len {}", c.len());
        assert!(c.len() > 32);

        let r = Response {
            status: Status::ItIsADirectory,
            message: None,
        }
        .serialize();
        println!("Response no msg {}", r.len());
        assert!(r.len() >= 32);
        let r = Response {
            status: Status::Ok,
            message: Some("hello".to_string()),
        }
        .serialize();
        assert!(r.len() >= 32);
        println!("Response with msg 5 {}", r.len());
        let head = FileHeader::serialize_direct(1234, "foo");
        println!("File Header {}", head.len());
        assert!(head.len() >= 32);
        let trail = FileTrailer::serialize_direct();
        println!("File Trailer {}", trail.len());
        assert!(trail.len() >= 16);
    }
}
