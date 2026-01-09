//! Set Metadata command
// (c) 2025 Ross Younger

use anyhow::Result;
use async_trait::async_trait;
use cfg_if::cfg_if;
use tokio::io::AsyncWriteExt;
use tracing::trace;

use crate::Parameters;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendingStream};
use crate::protocol::compat::Feature;
use crate::protocol::session::{Command, MetadataAttr, Response, SetMetadataArgs, Status};
use crate::session::handler::SessionCommandInner;
use crate::session::{RequestResult, error_and_return, handler::CommandHandler};
// Extension trait for std::fs::Metadata
use crate::util::FsMetadataExt as _;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

pub(crate) struct SetMetadataHandler;

#[async_trait]
impl CommandHandler for SetMetadataHandler {
    type Args = SetMetadataArgs;

    async fn send_impl<'a, S: SendingStream, R: ReceivingStream>(
        &mut self,
        inner: &mut SessionCommandInner<'a, S, R>,
        job: &crate::CopyJobSpec,
        _params: Parameters,
    ) -> Result<RequestResult> {
        anyhow::ensure!(
            inner.compat.supports(Feature::MKDIR_SETMETA_LS),
            "Operation not supported by remote"
        );

        let localmeta = tokio::fs::metadata(&job.source.filename).await?;
        anyhow::ensure!(
            localmeta.is_dir(),
            "SetMetadata currently only supports directories"
        );

        // This is a trivial operation, we do not bother with a progress bar.

        trace!("sending command");
        let mut outbound = &mut inner.stream.send;
        let cmd = Command::SetMetadata(SetMetadataArgs {
            path: job.destination.filename.clone(),
            metadata: localmeta.tagged_data_for_dir(inner.compat),
            options: vec![],
        });
        cmd.to_writer_async_framed(&mut outbound).await?;
        outbound.flush().await?;

        trace!("await response");
        let _ = Response::from_reader_async_framed(&mut inner.stream.recv)
            .await?
            .into_result()?;
        Ok(RequestResult::default())
    }

    async fn handle_impl<'a, S: SendingStream, R: ReceivingStream>(
        &mut self,
        inner: &mut SessionCommandInner<'a, S, R>,
        args: &SetMetadataArgs,
    ) -> Result<()> {
        let path = &args.path;
        let stream = &mut inner.stream;

        let localmeta = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) => error_and_return!(stream, e),
        };
        if !localmeta.is_dir() {
            error_and_return!(stream, Status::ItIsAFile);
        }

        for md in &args.metadata {
            match md.tag() {
                None | Some(MetadataAttr::Invalid) => (),
                Some(MetadataAttr::ModeBits) => {
                    static_assertions::assert_cfg!(
                        any(unix, windows),
                        "This OS is not currently supported"
                    );
                    cfg_if! {
                        if #[cfg(unix)] {
                            if let Some(mode) = md.data.as_unsigned_ref() {
                                let mode = (mode & 0o777) as u32;
                                let mut perms = localmeta.permissions();
                                perms.set_mode(mode);
                                if let Err(e)= tokio::fs::set_permissions(&path, perms).await {
                                    error_and_return!(stream, e);
                                }
                            }
                        } else if #[cfg(windows)] {
                            // The Windows 'read only' attribute is not useful on a directory. Ignore for now.
                            // TODO: Use NTFS permissions
                        }
                    }
                }
                Some(t) => anyhow::bail!("unknown metadata tag {t}"),
            }
        }
        crate::session::common::send_ok(&mut stream.send).await
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use anyhow::{Result, bail};
    use assertables::assert_contains;

    use crate::{
        Configuration, Parameters,
        client::CopyJobSpec,
        protocol::{
            control::Compatibility,
            session::Command,
            test_helpers::{new_test_plumbing, read_from_stream},
        },
        session::RequestResult,
    };
    use littertray::LitterTray;

    async fn test_setmeta_main(
        local_path: &str,
        remote_path: &str,
    ) -> Result<(Result<RequestResult>, Result<()>)> {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let spec =
            CopyJobSpec::from_parts(local_path, &format!("somehost:{remote_path}"), false, false)
                .unwrap();
        let params = Parameters::default();

        let (mut sender, _) = crate::session::factory::client_sender(
            pipe1,
            &spec,
            crate::session::factory::TransferPhase::Post,
            Compatibility::Level(4),
            &params,
            None,
            Configuration::system_default(),
        );
        let sender_fut = sender.send(&spec, params);
        tokio::pin!(sender_fut);

        let result = read_from_stream(&mut pipe2.recv, &mut sender_fut).await;
        let cmd = result.expect_left("sender should not have completed early")?;
        let Command::SetMetadata(_) = cmd else {
            bail!("expected SetMetadata command");
        };
        let (mut handler, _) = crate::session::factory::command_handler(
            pipe2,
            cmd,
            Compatibility::Level(4),
            Configuration::system_default(),
        );

        let (r1, r2) = tokio::join!(sender_fut, handler.handle());
        Ok((r1, r2))
    }

    #[tokio::test]
    async fn setmeta_success() -> Result<()> {
        LitterTray::try_with_async(async |tray| {
            #[cfg(unix)]
            use std::os::unix::fs::PermissionsExt as _;

            let _ = tray.make_dir("testdir")?;
            let _ = tray.make_dir("remote")?;
            let _ = tray.make_dir("remote/testdir")?;

            #[allow(unused_variables)]
            let target_mode = 0o500;
            #[allow(unused_variables)]
            let other_mode = 0o505;
            #[cfg(unix)]
            {
                // Set the perms differently on the two directories
                use std::fs::{metadata, set_permissions};
                let mut perms = metadata("testdir").unwrap().permissions();
                perms.set_mode(target_mode);
                set_permissions("testdir", perms).expect("failed to set testdir permissions");

                let mut perms = metadata("remote/testdir").unwrap().permissions();
                perms.set_mode(other_mode);
                set_permissions("remote/testdir", perms)
                    .expect("failed to set remote/testdir permissions");
            }

            let (r1, r2) = test_setmeta_main("testdir", "remote/testdir").await?;
            assert!(r1.is_ok());
            assert!(r2.is_ok());
            #[cfg(unix)]
            {
                let new_mode = std::fs::metadata("remote/testdir")
                    .expect("dir should exist")
                    .permissions()
                    .mode();
                assert_eq!(new_mode & 0o777, target_mode);
            }
            Ok(())
        })
        .await
    }
    #[tokio::test]
    async fn setmeta_file_not_found() -> Result<()> {
        LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("local")?;
            let _ = tray.make_dir("remote")?;
            let (r1, r2) = test_setmeta_main("local", "remote/xyzy").await?;
            let msg = r1.unwrap_err().to_string();
            assert_contains!(msg, "FileNotFound");
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }
}
