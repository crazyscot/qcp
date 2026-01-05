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
//! * In the original version of each struct, these must be sent as 0.
//! * A later version can quietly change the definition to `Option<something>`.
//!
//! This is on top of the general protocol extension trick of using unions (in Rust, enums with contents)
//! as described in section 4 of [BARE].
//! This must itself be used with care:  older versions of the software do not understand any newer enum values,
//! so would choke on them. This is why the control channel includes [`Compatibility` level](crate::protocol::control::Compatibility).
//!
//! [BARE]: https://www.ietf.org/archive/id/draft-devault-bare-11.html
//! [serde_bare]: https://docs.rs/serde_bare/latest/serde_bare/

use crate::util::io::read_available_non_blocking;
use anyhow::Error;
use bytes::BytesMut;
use serde_bare::error::Error as sbError;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/////////////////////////////////////////////////////////////////////////////////////////////
// STREAM TYPEDEFS

/// Marker trait for streams used for sending data
pub trait SendingStream: AsyncWrite + Send + Unpin {}
impl SendingStream for quinn::SendStream {}

#[cfg(test)]
impl SendingStream for tokio_test::io::Mock {}

/// Marker trait for streams used for receiving data
pub trait ReceivingStream: AsyncRead + Send + Unpin {}
impl ReceivingStream for quinn::RecvStream {}

#[cfg(test)]
impl ReceivingStream for tokio_test::io::Mock {}

/// Syntactic sugar helper type
#[derive(Debug)]
pub struct SendReceivePair<S: SendingStream, R: ReceivingStream> {
    /// outbound data
    pub send: S,
    /// inbound data
    pub recv: R,
}

impl<S: SendingStream, R: ReceivingStream> From<(S, R)> for SendReceivePair<S, R> {
    fn from(value: (S, R)) -> Self {
        Self {
            send: value.0,
            recv: value.1,
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////////
// WIRE MESSAGE FRAMING

/// Framing header used on the wire for protocol messages
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Eq, Debug, Default, Clone, Copy)]
pub struct MessageHeader {
    /// Size of the payload that follows the header
    pub size: u32,
}

impl MessageHeader {
    /// The on-wire size of this struct (of `MessageHeader` itself), which is fixed (any change would constitute a breaking protocol change)
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
    /// Specifies an absolute limit on the wire encoding of this type.
    /// The `from_..._framed` functions reject any attempts to deserialise
    /// a message with a header frame longer than the given value for the type.
    ///
    /// <div class="warning">
    /// This limit is important to prevent excessive memory consumption in the event of unforeseen bugs or network corruption,
    /// but set it with caution. The protocol is designed to have forwards compatibility, which these limits may break.
    /// The default limit is therefore somewhat permissive.
    /// </div>
    const WIRE_ENCODING_LIMIT: u32 = 1_048_576;

    /// Checks the passed-in limit against this type's [`WIRE_ENCODING_LIMIT`](Self::WIRE_ENCODING_LIMIT).
    ///
    /// # Return
    /// OK(()) iff the passed-in limit satisfies any requirement specified by this type
    fn check_size(size: u32) -> Result<(), Error> {
        Self::check_size_usize(size as usize)
    }

    /// Checks the passed-in limit against this type's [`WIRE_ENCODING_LIMIT`](Self::WIRE_ENCODING_LIMIT).
    ///
    /// # Return
    /// OK(()) iff the passed-in limit satisfies any requirement specified by this type
    fn check_size_usize(size: usize) -> Result<(), Error> {
        anyhow::ensure!(
            size <= Self::WIRE_ENCODING_LIMIT as usize,
            format!(
                "Wire message size {} was too long for {} (limit: {})",
                size,
                std::any::type_name::<Self>(),
                Self::WIRE_ENCODING_LIMIT
            )
        );
        Ok(())
    }

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
    ) -> impl Future<Output = Result<Self, Error>> + Send
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
    ///
    /// This function checks the struct's [`WIRE_ENCODING_LIMIT`](Self::WIRE_ENCODING_LIMIT).
    fn from_reader_framed<R>(reader: &mut R) -> Result<Self, Error>
    where
        R: std::io::Read,
    {
        let header = MessageHeader::from_reader(reader, MessageHeader::SIZE)?;
        Self::check_size(header.size)?;
        Self::from_reader(reader, header.size)
    }
    /// Deserializes this struct asynchronously from an arbitrary async reader by reading a [`MessageHeader`], then this struct as payload.
    ///
    /// This function checks the struct's [`WIRE_ENCODING_LIMIT`](Self::WIRE_ENCODING_LIMIT).
    fn from_reader_async_framed<R>(
        reader: &mut R,
    ) -> impl Future<Output = Result<Self, Error>> + Send
    where
        R: AsyncReadExt + std::marker::Unpin + Send,
    {
        async {
            let header = MessageHeader::from_reader_async(reader, MessageHeader::SIZE).await?;
            if let Err(e) = Self::check_size(header.size) {
                // Did we receive text on the remote stderr? We shouldn't have, but try to decode it and output.
                let mut raw = BytesMut::zeroed(256);
                let mut buf = tokio::io::ReadBuf::new(&mut raw);
                if let Ok(hdr) = header.to_vec() {
                    buf.put_slice(&hdr);
                    if read_available_non_blocking(reader, &mut buf).await.is_ok()
                        && let Ok(s) = str::from_utf8(&raw)
                    {
                        // If it's not valid UTF-8, this will fail to decode and we won't output it.
                        tracing::warn!("Received protocol garbage: {}", s.trim());
                    }
                }
                return Err(e);
            }
            Self::from_reader_async(reader, header.size).await
        }
    }

    /// Serializes this struct into an arbitrary writer by writing a [`MessageHeader`], then this struct as payload
    fn to_writer_framed<W>(&self, writer: &mut W) -> Result<(), Error>
    where
        W: std::io::Write,
    {
        let vec = self.to_vec()?;
        Self::check_size_usize(vec.len())?;
        #[allow(clippy::cast_possible_truncation)] // already checked
        let header = MessageHeader {
            size: vec.len() as u32,
        }
        .to_vec()?;
        writer.write_all(&header)?;
        Ok(writer.write_all(&vec)?)
    }

    /// Serializes this struct asynchronously into an arbitrary async writer by writing a [`MessageHeader`], then this struct as payload
    fn to_writer_async_framed<W>(
        &self,
        writer: &mut W,
    ) -> impl Future<Output = Result<(), Error>> + Send
    where
        W: AsyncWriteExt + std::marker::Unpin + Send,
    {
        async {
            let vec = self.to_vec()?;
            Self::check_size_usize(vec.len())?;
            #[allow(clippy::cast_possible_truncation)] // already checked
            let header = MessageHeader {
                size: vec.len() as u32,
            }
            .to_vec()?;
            writer.write_all(&header).await?;
            Ok(writer.write_all(&vec).await?)
        }
    }
}

/////////////////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::protocol::common::MessageHeader;

    use super::{Error, ProtocolMessage};
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};
    use std::io::Cursor;

    // Test struct implementing the trait
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct TestMessage {
        data: Vec<u8>,
    }

    impl ProtocolMessage for TestMessage {
        const WIRE_ENCODING_LIMIT: u32 = 16;
    }

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
    fn deserialize_limit() {
        let buf = [
            18, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let _ = TestMessage::from_reader_framed(&mut Cursor::new(buf))
            .expect_err("an error was expected");
    }

    #[test]
    fn serialize_limit() {
        #[allow(clippy::cast_possible_truncation)]
        let msg = TestMessage {
            data: vec![0u8; (TestMessage::WIRE_ENCODING_LIMIT + 1) as usize],
        };
        let mut buf = Vec::new();
        let _ = msg
            .to_writer_framed(&mut buf)
            .expect_err("an error was expected");
    }

    #[tokio::test]
    async fn deserialize_limit_async() {
        let buf = [
            18, 0, 0, 0, 17, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let _ = TestMessage::from_reader_async_framed(&mut Cursor::new(buf))
            .await
            .expect_err("an error was expected");
    }

    #[tokio::test]
    async fn serialize_limit_async() {
        #[allow(clippy::cast_possible_truncation)]
        let msg = TestMessage {
            data: vec![0u8; (TestMessage::WIRE_ENCODING_LIMIT + 1) as usize],
        };
        let mut buf = Vec::new();
        let _ = msg
            .to_writer_framed(&mut buf)
            .expect_err("an error was expected");
    }

    #[test]
    fn deserialize_junk_over_long() {
        // Edge cases above 2^32, to trap any signedness issues
        // (but without allocing a 4GB vec, i.e. more like a fuzz test)
        for testcase in &[1u32 << 31, 4_294_967_295 /* 2^32 - 1 */] {
            let buf = MessageHeader { size: *testcase }.to_vec().unwrap();
            let _ = TestMessage::from_reader_framed(&mut Cursor::new(buf))
                .expect_err("an error was expected");
        }
    }
    #[test]
    fn deserialize_junk_zero_data() {
        let buf = MessageHeader { size: 0 }.to_vec().unwrap();
        let _ = TestMessage::from_reader_framed(&mut Cursor::new(buf))
            .expect_err("an error was expected");
    }
    #[test]
    fn deserialize_junk_insufficient_data() {
        #![allow(clippy::cast_possible_truncation)]
        // The header is correct for the payload, but the payload is inconsistent.
        // This test relies on knowing how a TestMessage is encoded (Length, <data>)
        let mut bogus_payload = vec![10u8 /*length*/, 1, 2, 3 /* short data */];
        let mut buf = MessageHeader {
            size: bogus_payload.len() as u32,
        }
        .to_vec()
        .unwrap();
        buf.append(&mut bogus_payload);
        let _ = TestMessage::from_reader_framed(&mut Cursor::new(buf))
            .expect_err("an error was expected");
    }
}
