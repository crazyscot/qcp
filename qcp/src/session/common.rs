//! Common functions within the session protocol
// (c) 2024-5 Ross Younger

use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish};
use std::io::ErrorKind;
use tokio::io::AsyncWriteExt;

use crate::{
    client::{CopyJobSpec, progress::style_for},
    protocol::{
        common::ProtocolMessage as _,
        session::{Response, ResponseV1, Status},
    },
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

fn error_to_status(err: &anyhow::Error) -> (Status, Option<String>) {
    if let Some(st) = err.downcast_ref::<Status>() {
        (*st, None)
    } else if let Some(io) = err.downcast_ref::<std::io::Error>() {
        match io.kind() {
            ErrorKind::NotFound => (Status::FileNotFound, None),
            ErrorKind::PermissionDenied => (Status::IncorrectPermissions, None),
            ErrorKind::IsADirectory => (Status::ItIsADirectory, None),
            ErrorKind::StorageFull => (Status::DiskFull, None),
            _ => (Status::IoError, Some(io.to_string())),
        }
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

/// Adds a progress bar to the stack (in `MultiProgress`) for the current job
pub(super) fn progress_bar_for(
    display: &MultiProgress,
    job: &CopyJobSpec,
    filename_width: usize,
    steps: u64,
    quiet: bool,
) -> Result<ProgressBar> {
    if quiet {
        return Ok(ProgressBar::hidden());
    }
    let name = format!(
        "{:width$}",
        job.display_filename().to_string_lossy(),
        width = filename_width
    );
    Ok(display.add(
        ProgressBar::new(steps)
            .with_style(indicatif::ProgressStyle::with_template(style_for(
                filename_width,
            ))?)
            .with_message(name)
            .with_finish(ProgressFinish::Abandon),
    ))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::io::{self, Cursor};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    use indicatif::MultiProgress;
    use pretty_assertions::assert_eq;
    use tokio::io::AsyncWrite;

    use super::progress_bar_for;
    use crate::client::{CopyJobSpec, FileSpec};
    use crate::protocol::common::ProtocolMessage as _;
    use crate::protocol::session::{Response, ResponseV1, Status};

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
    fn test_progress_bar_for() {
        let display = MultiProgress::new();
        let job = CopyJobSpec {
            source: FileSpec {
                filename: "test_file.txt".to_string(),
                ..Default::default()
            },
            destination: FileSpec {
                filename: "dest.txt".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        // Test quiet mode
        let pb = progress_bar_for(&display, &job, 12, 100, true).unwrap();
        assert!(pb.is_hidden());
        assert_eq!(pb.length(), None);
        assert_eq!(pb.message(), "");

        // Test visible mode
        let pb = progress_bar_for(&display, &job, 12, 100, false).unwrap();
        // Checking is_hidden() isn't sound in a CI environment; if stderr isn't to a terminal, is_hidden() always returns true.
        // This can be provoked by rusty_fork_test.
        // But we can still assert about the length and message, which do work as expected (cf. the hidden progress bar above)
        assert_eq!(pb.length(), Some(100));
        assert_eq!(pb.message(), "test_file.txt");
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
