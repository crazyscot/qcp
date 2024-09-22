// QCP session wire protocol
// (c) 2024 Ross Younger

/*
 * The session protocol frames a QUIC (Quinn) bidirectional stream.
 * The protocol consists of Command and Response packets defined in schema/session.capnp.
 * Packets are sent using the standard capnp framing.
 *
 * Client -> Server: <initiates stream>
 * C -> S : Command packet
 * S -> C : Response packet
 * (Then they do whatever is appropriate for the command. See the notes in session.capnp.)
 */

pub mod session_capnp {
    include!(concat!(env!("OUT_DIR"), "/session_capnp.rs"));
}

use std::fmt::Display;

use anyhow::Result;
use capnp::message::ReaderOptions;
use session_capnp::Status;
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

#[derive(Debug, strum_macros::Display)]
pub enum Command {
    Get(GetArgs),
    Put(PutArgs),
}
#[derive(Debug)]
pub struct GetArgs {
    pub filename: String,
}
#[derive(Debug)]
pub struct PutArgs {
    pub filename: String,
}

impl Command {
    pub fn new_get(filename: &str) -> Self {
        Self::Get(GetArgs {
            filename: filename.to_string(),
        })
    }
    pub fn new_put(filename: &str) -> Self {
        Self::Put(PutArgs {
            filename: filename.to_string(),
        })
    }

    pub async fn write<W>(&self, write: &mut W) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin,
    {
        use crate::protocol::session::Command::*;
        let mut msg = ::capnp::message::Builder::new_default();
        let builder = msg.init_root::<session_capnp::command::Builder>();
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
        capnp_futures::serialize::write_message(write.compat_write(), &msg).await?;
        Ok(())
    }
    pub async fn read<R>(read: &mut R) -> Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        use session_capnp::command::{self, args::*};
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg: command::Reader = reader.get_root()?;

        Ok(match msg.get_args().which() {
            Ok(Get(get)) => Command::Get(GetArgs {
                filename: get?.get_filename()?.to_string()?,
            }),
            Ok(Put(put)) => Command::Put(PutArgs {
                filename: put?.get_filename()?.to_string()?,
            }),
            Err(e) => {
                anyhow::bail!("error reading command: {e}");
            }
        })
    }
}

#[derive(Debug)]
pub struct Response {
    pub status: Status,
    pub message: Option<String>,
}

impl Response {
    pub fn serialize(&self) -> Vec<u8> {
        Self::serialize_direct(self.status, self.message.as_deref())
    }
    pub fn serialize_direct(status: Status, message: Option<&str>) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();

        let mut response_msg = msg.init_root::<session_capnp::response::Builder>();
        response_msg.set_status(status);
        if let Some(s) = message {
            response_msg.set_message(s);
        }
        capnp::serialize::write_message_to_words(&msg)
    }
    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: session_capnp::response::Reader = reader.get_root()?;
        let status = msg_reader.get_status()?;
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
pub struct FileHeader {
    pub size: u64,
    pub filename: String,
}

impl FileHeader {
    pub fn serialize_direct(size: u64, filename: &str) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();

        let mut response_msg = msg.init_root::<session_capnp::file_header::Builder>();
        response_msg.set_size(size);
        response_msg.set_filename(filename);
        capnp::serialize::write_message_to_words(&msg)
    }
    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let msg_reader: session_capnp::file_header::Reader = reader.get_root()?;
        Ok(Self {
            size: msg_reader.get_size(),
            filename: msg_reader.get_filename()?.to_string()?,
        })
    }
}

#[derive(Debug)]
pub struct FileTrailer {}

impl FileTrailer {
    pub fn serialize_direct() -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();

        let mut _response_msg = msg.init_root::<session_capnp::file_trailer::Builder>();
        capnp::serialize::write_message_to_words(&msg)
    }
    pub async fn read<R>(read: &mut R) -> anyhow::Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin,
    {
        let reader =
            capnp_futures::serialize::read_message(read.compat(), ReaderOptions::new()).await?;
        let _msg_reader: session_capnp::file_trailer::Reader = reader.get_root()?;
        Ok(Self {})
    }
}