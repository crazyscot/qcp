//! Handler for a single incoming stream on a connection
// (c) 2024 Ross Younger

use crate::protocol::common::{
    ProtocolMessage as _, ReceivingStream, SendReceivePair, SendingStream,
};
use crate::protocol::control::Compatibility;
use crate::protocol::display_vec_td;
use crate::protocol::session::Command;

use tracing::{Instrument as _, trace, trace_span};

pub(super) async fn handle_stream<W, R>(
    mut sp: SendReceivePair<W, R>,
    compat: Compatibility,
    io_buffer_size: u64,
) -> anyhow::Result<()>
where
    R: ReceivingStream + 'static, // AsyncRead + Unpin + Send,
    W: SendingStream + 'static,   // AsyncWrite + Unpin + Send,
{
    use crate::session;
    trace!("reading command");
    let packet = Command::from_reader_async_framed(&mut sp.recv).await?;

    let (span, mut handler) = match packet {
        Command::Get(args) => (
            trace_span!("SERVER:GET", filename = args.filename.clone()),
            session::Get::boxed(sp, Some(args.into()), compat),
        ),
        Command::Put(args) => (
            trace_span!("SERVER:PUT", filename = args.filename.clone()),
            session::Put::boxed(sp, Some(args.into()), compat),
        ),
        Command::Get2(args) => (
            trace_span!("SERVER:GET2", filename = args.filename.clone()),
            session::Get::boxed(sp, Some(args), compat),
        ),
        Command::Put2(args) => (
            trace_span!("SERVER:PUT2", filename = args.filename.clone()),
            session::Put::boxed(sp, Some(args), compat),
        ),
        Command::CreateDirectory(args) => (
            trace_span!("SERVER:MKDIR", filename = args.dir_name.clone()),
            session::CreateDirectory::boxed(sp, Some(args), compat),
        ),
        Command::SetMetadata(args) => (
            trace_span!(
                "SERVER:SETMETA",
                filename = args.path.clone(),
                metadata = display_vec_td(&args.metadata),
            ),
            session::SetMetadata::boxed(sp, Some(args), compat),
        ),
    };

    handler.handle(io_buffer_size).instrument(span).await
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::protocol::{
        common::{ProtocolMessage, SendReceivePair},
        control::Compatibility,
        session::{Command, GetArgs, PutArgs, Response, ResponseV1, Status},
    };
    use crate::util::io::DEFAULT_COPY_BUFFER_SIZE;

    use super::handle_stream;
    use tokio::io::simplex;
    use tokio_test::io::Builder;

    #[tokio::test]
    async fn test_handle_get_command() {
        let get_cmd = Command::Get(GetArgs {
            filename: "no-such-file.txt".to_string(),
        });
        let mut send_buf = Vec::new();
        get_cmd.to_writer_framed(&mut send_buf).unwrap();

        let mock_recv = Builder::new()
            .read(&send_buf[0..4])
            .read(&send_buf[4..])
            .build();

        let (mut out_read, out_write) = simplex(1024);

        handle_stream(
            SendReceivePair::from((out_write, mock_recv)),
            Compatibility::Level(1),
            DEFAULT_COPY_BUFFER_SIZE,
        )
        .await
        .unwrap();

        let resp = Response::from_reader_async_framed(&mut out_read)
            .await
            .unwrap();

        let Response::V1(ResponseV1 { status, .. }) = resp;
        assert_eq!(status, Status::FileNotFound);
        // message is OS specific, so we don't check it here
    }

    #[tokio::test]
    async fn test_handle_put_command() {
        let put_cmd = Command::Put(PutArgs {
            filename: "/fjds/no-such-file.txt".to_string(),
        });
        let mut send_buf = Vec::new();
        put_cmd.to_writer_framed(&mut send_buf).unwrap();

        let expected = Response::V1(ResponseV1::new(Status::DirectoryDoesNotExist.into(), None));
        let mut expect_buf = Vec::new();
        expected.to_writer_framed(&mut expect_buf).unwrap();

        let mock_recv = Builder::new()
            .read(&send_buf[0..4])
            .read(&send_buf[4..])
            .build();
        let mock_send = Builder::new()
            .write(&expect_buf[0..4])
            .write(&expect_buf[4..])
            .build();

        let result = handle_stream(
            SendReceivePair::from((mock_send, mock_recv)),
            Compatibility::Level(1),
            DEFAULT_COPY_BUFFER_SIZE,
        )
        .await;
        assert!(result.is_ok());
    }
}
