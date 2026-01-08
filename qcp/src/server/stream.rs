//! Handler for a single incoming stream on a connection
// (c) 2024 Ross Younger

use std::io::ErrorKind;

use crate::protocol::common::{
    ProtocolMessage as _, ReceivingStream, SendReceivePair, SendingStream,
};
use crate::protocol::control::Compatibility;
use crate::protocol::session::Command;

use tracing::{Instrument as _, trace, trace_span};

pub(crate) async fn handle_stream<W, R>(
    mut sp: SendReceivePair<W, R>,
    compat: Compatibility,
    config: &crate::config::Configuration,
) -> anyhow::Result<()>
where
    R: ReceivingStream + 'static, // AsyncRead + Unpin + Send,
    W: SendingStream + 'static,   // AsyncWrite + Unpin + Send,
{
    use crate::session;
    trace!("reading command");
    let packet = match Command::from_reader_async_framed(&mut sp.recv).await {
        Ok(c) => c,
        Err(e) => {
            if let Some(ee) = e.downcast_ref::<std::io::Error>()
                && ee.kind() == ErrorKind::UnexpectedEof
            {
                tracing::debug!("handler got EOF while awaiting command; bailing");
                return Ok(());
            }
            return Err(e);
        }
    };

    let (mut handler, span_info) = session::factory::command_handler(sp, packet, compat, config);
    let span = trace_span!(
        "handler",
        cmd = span_info.name,
        filename = span_info.primary_arg
    );
    handler.handle().instrument(span).await
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::{
        Configuration,
        protocol::{
            common::{ProtocolMessage, SendReceivePair},
            control::Compatibility,
            session::{
                Command, CreateDirectoryArgs, Get2Args, GetArgs, Put2Args, PutArgs, Response,
                ResponseV1, SetMetadataArgs, Status,
            },
        },
    };

    use super::handle_stream;
    use littertray::LitterTray;
    use tokio::io::simplex;
    use tokio_test::io::Builder;

    async fn test_handler(cmd: Command, compat: u16) -> ResponseV1 {
        let mut send_buf = Vec::new();
        cmd.to_writer_framed(&mut send_buf).unwrap();

        let mock_recv = Builder::new()
            .read(&send_buf[0..4])
            .read(&send_buf[4..])
            .build();

        let (mut out_read, out_write) = simplex(1024);

        handle_stream(
            SendReceivePair::from((out_write, mock_recv)),
            Compatibility::Level(compat),
            Configuration::system_default(),
        )
        .await
        .unwrap();

        let resp = Response::from_reader_async_framed(&mut out_read)
            .await
            .unwrap();
        let Response::V1(r) = resp;
        r
    }

    #[tokio::test]
    async fn test_handle_get() {
        let cmd = Command::Get(GetArgs {
            filename: "no-such-file.txt".to_string(),
        });
        let resp = test_handler(cmd, 1).await;
        assert_eq!(resp.status, Status::FileNotFound);
        // message is OS specific, so we don't check it here
    }

    #[tokio::test]
    async fn test_handle_put() {
        let cmd = Command::Put(PutArgs {
            filename: "/fjds/no-such-file.txt".to_string(),
        });
        let resp = test_handler(cmd, 1).await;
        assert_eq!(resp.status, Status::DirectoryDoesNotExist);
    }

    #[tokio::test]
    async fn handle_get2() {
        let cmd = Command::Get2(Get2Args {
            filename: String::from("/no-such-file"),
            ..Default::default()
        });
        let resp = test_handler(cmd, 3).await;
        assert_eq!(resp.status, Status::FileNotFound);
    }
    #[tokio::test]
    async fn handle_put2() {
        let cmd = Command::Put2(Put2Args {
            filename: String::from("/blah/no-such-file"),
            ..Default::default()
        });
        let resp = test_handler(cmd, 3).await;
        assert_eq!(resp.status, Status::DirectoryDoesNotExist);
    }
    #[tokio::test]
    async fn handle_mkdir() {
        let resp = LitterTray::try_with_async(async |_| {
            let cmd = Command::CreateDirectory(CreateDirectoryArgs {
                dir_name: String::from("my_dir"),
                ..Default::default()
            });
            Ok(test_handler(cmd, 3).await)
        })
        .await
        .unwrap();
        assert_eq!(resp.status, Status::Ok);
    }
    #[tokio::test]
    async fn handle_setmeta() {
        let cmd = Command::SetMetadata(SetMetadataArgs {
            path: String::from("nosuchdir"),
            ..Default::default()
        });
        let resp = test_handler(cmd, 3).await;
        assert_eq!(resp.status, Status::FileNotFound);
    }
}
