// (c) 2025 Ross Younger

//! Common functions and definitions shared by the [control](super::control) and [session](super::session) protocols
//!
//! # On-Wire Framing
//!
//! All protocol messages are sent in two parts:
//!
//! * [`MessageHeader`]
//! * The encoded message
//!
//! Both the header and payload are encoded using [BARE].
//!
//! # Note about protocol extensibility
//!
//! Some of the structures in these protocols have a trailing `extension: u8`.
//! This allows us to add new, optional fields later without a protocol break.
//! * In v0 of each struct, these must be sent as 0.
//! * A later version can quietly change the definition to `Option<something>`.
//!
//! This is on top of the general protocol extension trick of using unions (in Rust, enums with contents)
//! as described in section 4 of [BARE].
//! * The downside of this arrangement is that older versions of the software do not understand the newer enum,
//!   so would choke on it. Nevertheless this scheme is workable, provided there was some sort of protocol
//!   negotiation phase.
//!
//! [BARE]: https://www.ietf.org/archive/id/draft-devault-bare-11.html
//! [serde_bare]: https://docs.rs/serde_bare/latest/serde_bare/

use anyhow::Error;
use bytes::BytesMut;
use serde_bare::error::Error as sbError;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Syntactic sugar helper type
pub(crate) struct SendReceivePair<S, R> {
    /// outbound data
    pub send: S,
    /// inbound data
    pub recv: R,
}

impl<S, R> From<(S, R)> for SendReceivePair<S, R> {
    fn from(value: (S, R)) -> Self {
        Self {
            send: value.0,
            recv: value.1,
        }
    }
}

/// Syntactic sugar helper type for working with Quinn
pub(crate) type StreamPair = SendReceivePair<quinn::SendStream, quinn::RecvStream>;

/// Framing header used on the wire for protocol messages
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Default, Clone, Copy)]
pub struct MessageHeader {
    /// Size of the payload that follows the header
    pub size: u32,
}

impl MessageHeader {
    /// The on-wire size of this struct, which is fixed (any change would constitute a breaking protocol change)
    pub const SIZE: u32 = 4;
}

impl ProtocolMessage for MessageHeader {}

/// Provides I/O functions for all structs taking part in our protocol.
///
/// Callers are expected to use the `..._framed` functions, which include framing.
///
/// N.B. Message structs are not expected to override the provided implementations.
pub trait ProtocolMessage
where
    Self: serde::Serialize + serde::de::DeserializeOwned + Sync,
{
    /// Creates this struct from a slice of bytes.
    /// The slice must be the correct size for the payload (that's what [`MessageHeader`] is for).
    fn from_slice(slice: &[u8]) -> Result<Self, sbError> {
        serde_bare::from_slice(slice)
    }
    /// Deserializes this struct using a given number of bytes from an arbitrary reader.
    ///
    /// Of course you have to know how many bytes to read, but that's what [`MessageHeader`] is for.
    fn from_reader<R>(reader: &mut R, size: u32) -> Result<Self, Error>
    where
        R: std::io::Read,
    {
        let mut buffer = BytesMut::zeroed(size.try_into().unwrap());
        reader.read_exact(&mut buffer)?;
        Ok(serde_bare::from_slice(&buffer)?)
    }
    /// Deserializes this struct asynchronously using a given number of bytes from an async reader.
    ///
    /// Of course you have to know how many bytes to read, but that's what [`MessageHeader`] is for.
    fn from_reader_async<R>(
        reader: &mut R,
        size: u32,
    ) -> impl std::future::Future<Output = Result<Self, Error>> + Send
    where
        R: AsyncReadExt + std::marker::Unpin + Send,
    {
        async move {
            let mut buffer = BytesMut::zeroed(size.try_into().unwrap());
            let _ = reader.read_exact(&mut buffer).await?;
            Ok(serde_bare::from_slice(&buffer)?)
        }
    }

    /// Serializes this struct into a vector of bytes
    fn to_vec(&self) -> Result<Vec<u8>, sbError> {
        serde_bare::to_vec(&self)
    }

    /// Deserializes this struct from an arbitrary reader by reading a [`MessageHeader`], then this struct as payload.
    fn from_reader_framed<R>(reader: &mut R) -> Result<Self, Error>
    where
        R: std::io::Read,
    {
        let header = MessageHeader::from_reader(reader, MessageHeader::SIZE)?;
        Self::from_reader(reader, header.size)
    }
    /// Deserializes this struct asynchronously from an arbitrary async reader by reading a [`MessageHeader`], then this struct as payload.
    fn from_reader_async_framed<R>(
        reader: &mut R,
    ) -> impl std::future::Future<Output = Result<Self, Error>> + Send
    where
        R: AsyncReadExt + std::marker::Unpin + Send,
    {
        async {
            let header = MessageHeader::from_reader_async(reader, MessageHeader::SIZE).await?;
            Self::from_reader_async(reader, header.size).await
        }
    }

    /// Serializes this struct into an arbitrary writer by writing a [`MessageHeader`], then this struct as payload
    fn to_writer_framed<W>(&self, writer: &mut W) -> Result<(), Error>
    where
        W: std::io::Write,
    {
        let vec = self.to_vec()?;
        let header = MessageHeader {
            size: vec.len().try_into()?,
        }
        .to_vec()?;
        writer.write_all(&header)?;
        Ok(writer.write_all(&vec)?)
    }

    /// Serializes this struct asynchronously into an arbitrary async writer by writing a [`MessageHeader`], then this struct as payload
    fn to_writer_async_framed<W>(
        &self,
        writer: &mut W,
    ) -> impl std::future::Future<Output = Result<(), Error>> + Send
    where
        W: AsyncWriteExt + std::marker::Unpin + Send,
    {
        async {
            let vec = self.to_vec()?;
            let header = MessageHeader {
                size: vec.len().try_into()?,
            }
            .to_vec()?;
            writer.write_all(&header).await?;
            Ok(writer.write_all(&vec).await?)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Error, ProtocolMessage};
    use serde::{Deserialize, Serialize};
    use std::io::Cursor;

    // Test struct implementing the trait
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct TestMessage {
        data: Vec<u8>,
    }

    impl ProtocolMessage for TestMessage {}

    #[test]
    fn test_sync_framed_roundtrip() -> Result<(), Error> {
        let msg = TestMessage {
            data: vec![1, 2, 3],
        };
        let mut buf = Vec::new();
        msg.to_writer_framed(&mut buf)?;

        let decoded = TestMessage::from_reader_framed(&mut Cursor::new(buf))?;
        assert_eq!(msg, decoded);
        Ok(())
    }

    #[tokio::test]
    async fn test_async_framed_roundtrip() -> Result<(), Error> {
        let msg = TestMessage {
            data: vec![1, 2, 3],
        };
        let mut buf = Vec::new();
        msg.to_writer_async_framed(&mut buf).await?;

        // this is really curious. it seems to encode the vec without a length. So [1,2,3] -> len 1, bytes [2].
        // but the sync version works. what the heck?
        let decoded = TestMessage::from_reader_async_framed(&mut Cursor::new(buf)).await?;
        assert_eq!(msg, decoded);
        Ok(())
    }

    #[test]
    fn test_slicing() {
        let msg = TestMessage {
            data: vec![4, 5, 6],
        };
        let vec = msg.to_vec().unwrap();
        let decoded = TestMessage::from_slice(&vec).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_stream_pair() {
        type MyPair = super::SendReceivePair<i32, i32>;
        let input = (12, 34);
        let output = MyPair::from(input);
        assert_eq!(output.send, 12);
        assert_eq!(output.recv, 34);
    }
}
