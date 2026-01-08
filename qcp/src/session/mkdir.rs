//! Create Directory command
// (c) 2025 Ross Younger

use anyhow::Result;
use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tracing::{debug, trace};

use crate::Parameters;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::session::{Command, CreateDirectoryArgs, Response, Status};
use crate::session::common::send_ok;
use crate::session::{RequestResult, error_and_return, handler::CommandHandler};

pub(crate) struct CreateDirectoryHandler;

#[async_trait]
impl CommandHandler for CreateDirectoryHandler {
    type Args = CreateDirectoryArgs;

    async fn send_impl<S: SendingStream, R: ReceivingStream>(
        &mut self,
        stream: &mut SendReceivePair<S, R>,
        compat: crate::protocol::control::Compatibility,
        job: &crate::client::CopyJobSpec,
        _display: indicatif::MultiProgress,
        _filename_width: usize,
        _spinner: indicatif::ProgressBar,
        _config: &crate::config::Configuration,
        _params: Parameters,
    ) -> Result<RequestResult> {
        anyhow::ensure!(
            compat.supports(crate::protocol::compat::Feature::MKDIR_SETMETA_LS),
            "Operation not supported by remote"
        );
        anyhow::ensure!(
            job.destination.user_at_host.is_some(),
            "logic error: mkdir called for local destination"
        );

        trace!("sending command");
        let mut outbound = &mut stream.send;
        let cmd = Command::CreateDirectory(CreateDirectoryArgs {
            dir_name: job.destination.filename.clone(),
            options: vec![],
        });
        cmd.to_writer_async_framed(&mut outbound).await?;
        outbound.flush().await?;

        trace!("await response");
        let _ = Response::from_reader_async_framed(&mut stream.recv)
            .await?
            .into_result()?;
        Ok(RequestResult::default())
    }

    async fn handle_impl<S: SendingStream, R: ReceivingStream>(
        &mut self,
        _compat: crate::protocol::control::Compatibility,
        args: &CreateDirectoryArgs,
        _io_buffer_size: u64,
        stream: &mut SendReceivePair<S, R>,
    ) -> Result<()> {
        let path = &args.dir_name;

        let meta = tokio::fs::metadata(&path).await;
        if let Ok(meta) = meta {
            if meta.is_file() {
                error_and_return!(stream, Status::ItIsAFile);
            }
            if meta.is_dir() {
                // it already exists: this is not an error.
            } else {
                anyhow::bail!("mkdir: existing entity {path:?} is neither file nor directory");
            }
        } else {
            let result = tokio::fs::create_dir(path).await;
            if let Err(e) = result {
                let str = e.to_string();
                debug!("Could not mkdir: {str}");
                error_and_return!(stream, e);
            }
        }
        send_ok(&mut stream.send).await
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::CreateDirectoryHandler;
    use crate::{
        Configuration, Parameters,
        client::CopyJobSpec,
        protocol::{
            control::Compatibility,
            session::{Command, Status},
            test_helpers::{new_test_plumbing, read_from_stream},
        },
        session::{RequestResult, handler::SessionCommand},
        util::io::DEFAULT_COPY_BUFFER_SIZE,
    };
    use anyhow::{Result, bail};
    use littertray::LitterTray;
    use std::io::ErrorKind;

    async fn test_mkdir_main(path: &str) -> Result<(Result<RequestResult>, Result<()>)> {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let spec =
            CopyJobSpec::from_parts(path, &format!("somehost:{path}"), false, false).unwrap();
        let mut sender =
            SessionCommand::boxed(pipe1, None, Compatibility::Level(4), CreateDirectoryHandler);
        let params = Parameters::default();
        let sender_fut = sender.send(
            &spec,
            indicatif::MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden()),
            10,
            indicatif::ProgressBar::hidden(),
            Configuration::system_default(),
            params,
        );
        tokio::pin!(sender_fut);

        let result = read_from_stream(&mut pipe2.recv, &mut sender_fut).await;
        let cmd = result.expect_left("sender should not have completed early")?;
        let Command::CreateDirectory(args) = cmd else {
            bail!("expected CreateDirectory command");
        };

        let mut handler = SessionCommand::boxed(
            pipe2,
            Some(args),
            Compatibility::Level(4),
            CreateDirectoryHandler,
        );
        let (r1, r2) = tokio::join!(sender_fut, handler.handle(DEFAULT_COPY_BUFFER_SIZE));
        Ok((r1, r2))
    }

    async fn is_dir(path: &str) -> Result<bool> {
        let res = tokio::fs::metadata(path).await;
        if res.as_ref().is_err_and(|e| e.kind() == ErrorKind::NotFound) {
            return Ok(false);
        }
        Ok(res?.is_dir())
    }

    #[tokio::test]
    async fn mkdir_success() -> Result<()> {
        LitterTray::try_with_async(async |_| {
            let (r1, r2) = test_mkdir_main("d").await?;
            assert!(r1.is_ok());
            assert!(r2.is_ok());
            assert!(is_dir("d").await.expect("is_dir failed"));
            Ok(())
        })
        .await
    }
    #[tokio::test]
    async fn mkdir_missing_parent() -> Result<()> {
        LitterTray::try_with_async(async |_| {
            let (r1, r2) = test_mkdir_main("d/e").await?;
            assert!(r2.is_ok());
            let err = r1.expect_err("r1 should have errored");
            let st = Status::from(err);
            assert_eq!(st, Status::FileNotFound); // TODO: This should really be DirectoryDoesNotExist
            Ok(())
        })
        .await
    }
    #[tokio::test]
    async fn mkdir_directory_already_exists() -> Result<()> {
        LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("d")?;
            let _ = tray.make_dir("d/e")?;
            let (r1, r2) = test_mkdir_main("d/e").await?;
            eprintln!("{r1:?}");
            assert!(r1.is_ok());
            assert!(r2.is_ok());
            assert!(is_dir("d/e").await.expect("is_dir failed"));
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn mkdir_file_exists() -> Result<()> {
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("f", "ff")?;
            let (r1, r2) = test_mkdir_main("f").await?;
            let msg = r1.unwrap_err().to_string();
            assert_eq!(msg, "ItIsAFile");
            assert!(r2.is_ok());
            let rr = is_dir("f").await;
            assert!(rr.is_ok_and(|b| !b));
            Ok(())
        })
        .await
    }
}
