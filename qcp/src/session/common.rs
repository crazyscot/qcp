//! Common functions within the session protocol
// (c) 2024-5 Ross Younger

use std::io::ErrorKind;
use tokio::io::AsyncWriteExt;

use crate::protocol::{
    common::ProtocolMessage as _,
    session::{Response, ResponseV1, Status},
    {TaggedData, Variant},
};

/// Sends a response message
async fn send_response<W>(send: &mut W, status: Status, message: Option<&str>) -> anyhow::Result<()>
where
    W: AsyncWriteExt + std::marker::Unpin + Send,
{
    Response::V1(ResponseV1::new(
        status.into(),
        message.map(std::string::ToString::to_string),
    ))
    .to_writer_async_framed(send)
    .await
}

/// Helper function for sending an OK response
pub(super) async fn send_ok<W>(send: &mut W) -> anyhow::Result<()>
where
    W: AsyncWriteExt + std::marker::Unpin + Send,
{
    Response::V1(ResponseV1::new(Status::Ok.into(), None))
        .to_writer_async_framed(send)
        .await
}

pub(crate) fn io_error_to_status(io: &std::io::Error) -> (Status, Option<String>) {
    match io.kind() {
        ErrorKind::NotFound => (Status::FileNotFound, None),
        ErrorKind::PermissionDenied => (Status::IncorrectPermissions, None),
        ErrorKind::IsADirectory => (Status::ItIsADirectory, None),
        ErrorKind::StorageFull => (Status::DiskFull, None),
        _ => (Status::IoError, Some(io.to_string())),
    }
}

pub(crate) fn error_to_status(err: &anyhow::Error) -> (Status, Option<String>) {
    if let Some(st) = err.downcast_ref::<Status>() {
        (*st, None)
    } else if let Some(io) = err.downcast_ref::<std::io::Error>() {
        io_error_to_status(io)
    } else {
        (Status::UnknownError, Some(err.to_string()))
    }
}

/// Helper function for sending a Response from an Error
pub(super) async fn send_error<W>(send: &mut W, err: &anyhow::Error) -> anyhow::Result<()>
where
    W: AsyncWriteExt + std::marker::Unpin + Send,
{
    let (st, msg) = error_to_status(err);
    send_response(send, st, msg.as_deref()).await
}

/// Extension trait for finding options in a tagged data list
pub(crate) trait FindOption {
    /// Find an option by tag, returning the associated variant data if found
    fn find_option(&self, tag: crate::protocol::session::CommandParam) -> Option<&Variant>;
}

impl FindOption for Vec<TaggedData<crate::protocol::session::CommandParam>> {
    fn find_option(&self, tag: crate::protocol::session::CommandParam) -> Option<&Variant> {
        use crate::protocol::FindTag as _;
        self.find_tag(tag)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::protocol::common::ProtocolMessage as _;
    use crate::protocol::session::{Response, ResponseV1, Status};
    use pretty_assertions::assert_eq;
    use std::io::{self, Cursor};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::AsyncWrite;

    struct TestWriter {
        written: Vec<u8>,
    }

    impl TestWriter {
        fn new() -> Self {
            Self {
                written: Vec::new(),
            }
        }
    }

    impl AsyncWrite for TestWriter {
        fn poll_write(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            self.written.extend_from_slice(buf);
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn test_send_response() {
        let mut ts = TestWriter::new();
        super::send_response(&mut ts, Status::Ok, Some("hello"))
            .await
            .unwrap();

        // unpick it...
        let msg = Response::from_reader_framed(&mut Cursor::new(ts.written)).unwrap();
        assert_eq!(
            msg,
            Response::V1(ResponseV1::new(
                Status::Ok.into(),
                Some("hello".to_string())
            ))
        );
    }

    #[test]
    fn unknown_error_status() {
        #[derive(thiserror::Error, Debug, derive_more::Display)]
        #[display("the answer is {_0}")]
        struct MyError(u32);

        let (st, msg) = super::error_to_status(&anyhow::anyhow!(MyError(42)));
        assert_eq!(st, Status::UnknownError);
        assert_eq!(msg.unwrap(), "the answer is 42");
    }
}
