//! Set Metadata command
// (c) 2025 Ross Younger

use anyhow::Result;
use async_trait::async_trait;
use cfg_if::cfg_if;
use tokio::io::AsyncWriteExt;
use tracing::trace;

use super::{CommandStats, SessionCommandImpl, error_and_return};

use crate::Parameters;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::compat::Feature;
use crate::protocol::control::Compatibility;
use crate::protocol::session::{Command, MetadataAttr, Response, SetMetadataArgs, Status};

// Extension trait for std::fs::Metadata
use crate::util::FsMetadataExt as _;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

pub(crate) struct SetMetadata<S: SendingStream, R: ReceivingStream> {
    stream: SendReceivePair<S, R>,
    args: Option<SetMetadataArgs>,
    compat: Compatibility, // Selected compatibility level for the command
}

/// Boxing constructor
impl<S: SendingStream + 'static, R: ReceivingStream + 'static> SetMetadata<S, R> {
    pub(crate) fn boxed(
        stream: SendReceivePair<S, R>,
        args: Option<SetMetadataArgs>,
        compat: Compatibility,
    ) -> Box<dyn SessionCommandImpl> {
        Box::new(Self {
            stream,
            args,
            compat,
        })
    }
}

#[async_trait]
impl<S: SendingStream, R: ReceivingStream> SessionCommandImpl for SetMetadata<S, R> {
    async fn send(
        &mut self,
        job: &crate::CopyJobSpec,
        _display: indicatif::MultiProgress,
        _filename_width: usize,
        _spinner: indicatif::ProgressBar,
        _config: &crate::Configuration,
        _params: Parameters,
    ) -> Result<CommandStats> {
        anyhow::ensure!(
            self.compat.supports(Feature::MKDIR_SETMETA),
            "Operation not supported by remote"
        );

        let localmeta = tokio::fs::metadata(&job.source.filename).await?;
        anyhow::ensure!(
            localmeta.is_dir(),
            "SetMetadata currently only supports directories"
        );

        // This is a trivial operation, we do not bother with a progress bar.

        trace!("sending command");
        let mut outbound = &mut self.stream.send;
        let cmd = Command::SetMetadata(SetMetadataArgs {
            path: job.destination.filename.clone(),
            metadata: localmeta.tagged_data_for_dir(self.compat),
            options: vec![],
        });
        cmd.to_writer_async_framed(&mut outbound).await?;
        outbound.flush().await?;

        trace!("await response");
        let _ = Response::from_reader_async_framed(&mut self.stream.recv)
            .await?
            .into_result()?;
        Ok(CommandStats::default())
    }

    async fn handle(&mut self, _io_buffer_size: u64) -> Result<()> {
        let Some(ref args) = self.args else {
            anyhow::bail!("SETMETA handler called without args");
        };
        let path = &args.path;

        let localmeta = match tokio::fs::metadata(&path).await {
            Ok(m) => m,
            Err(e) => error_and_return!(self, e),
        };
        if !localmeta.is_dir() {
            error_and_return!(self, Status::ItIsAFile);
        }

        for md in &args.metadata {
            match md.tag() {
                None | Some(MetadataAttr::Invalid) => (),
                Some(MetadataAttr::ModeBits) => {
                    let mut perms = localmeta.permissions();
                    if let Some(mode) = md.data.as_unsigned_ref() {
                        let mode = (mode & 0o777) as u32;
                        static_assertions::assert_cfg!(
                            any(unix, windows),
                            "This OS is not currently supported"
                        );
                        cfg_if! {
                            if #[cfg(unix)] {
                                perms.set_mode(mode);
                                if let Err(e)= tokio::fs::set_permissions(&path, perms).await {
                                    error_and_return!(self, e);
                                }
                            } else if #[cfg(windows)] {
                                // The Windows 'read only' attribute is not useful on a directory. Ignore for now.
                                // TODO: Use NTFS permissions
                            }
                        }
                    }
                }
                Some(t) => anyhow::bail!("unknown metadata tag {t}"),
            }
        }
        crate::session::common::send_ok(&mut self.stream.send).await
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
        session::{CommandStats, SetMetadata},
        util::io::DEFAULT_COPY_BUFFER_SIZE,
    };
    use littertray::LitterTray;

    async fn test_setmeta_main(
        local_path: &str,
        remote_path: &str,
    ) -> Result<(Result<CommandStats>, Result<()>)> {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let spec =
            CopyJobSpec::from_parts(local_path, &format!("somehost:{remote_path}"), false, false)
                .unwrap();
        let mut sender = SetMetadata::boxed(pipe1, None, Compatibility::Level(4));
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
        let Command::SetMetadata(args) = cmd else {
            bail!("expected SetMetadata command");
        };

        let mut handler = SetMetadata::boxed(pipe2, Some(args), Compatibility::Level(4));
        let (r1, r2) = tokio::join!(sender_fut, handler.handle(DEFAULT_COPY_BUFFER_SIZE));
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

            let target_mode = 0o500;
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
