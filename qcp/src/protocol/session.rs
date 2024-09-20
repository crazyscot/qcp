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

pub const COMMAND_RESPONSE_MAX_LENGTH: u16 = 1024;

use std::fmt::Display;

use capnp::message::ReaderOptions;
use quinn::RecvStream;
use session_capnp::Status;
use tokio_util::compat::Compat as tokCompat;

pub enum Command {
    Get(GetArgs),
    Put(PutArgs),
}
pub struct GetArgs {
    pub filename: String,
}
pub struct PutArgs {
    pub filename: String,
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
    pub async fn read(read: &mut tokCompat<RecvStream>) -> anyhow::Result<Self> {
        let reader = capnp_futures::serialize::read_message(read, ReaderOptions::new()).await?;
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
    pub fn serialize(&self) -> Vec<u8> {
        Self::serialize_direct(self.size, &self.filename)
    }
    pub fn serialize_direct(size: u64, filename: &str) -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();

        let mut response_msg = msg.init_root::<session_capnp::file_header::Builder>();
        response_msg.set_size(size);
        response_msg.set_filename(filename);
        capnp::serialize::write_message_to_words(&msg)
    }
    pub async fn read(read: &mut tokCompat<RecvStream>) -> anyhow::Result<Self> {
        let reader = capnp_futures::serialize::read_message(read, ReaderOptions::new()).await?;
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
    pub fn serialize(&self) -> Vec<u8> {
        Self::serialize_direct()
    }
    pub fn serialize_direct() -> Vec<u8> {
        let mut msg = ::capnp::message::Builder::new_default();

        let mut _response_msg = msg.init_root::<session_capnp::file_trailer::Builder>();
        capnp::serialize::write_message_to_words(&msg)
    }
    pub async fn read(read: &mut tokCompat<RecvStream>) -> anyhow::Result<Self> {
        let reader = capnp_futures::serialize::read_message(read, ReaderOptions::new()).await?;
        let _msg_reader: session_capnp::file_trailer::Reader = reader.get_root()?;
        Ok(Self {})
    }
}
