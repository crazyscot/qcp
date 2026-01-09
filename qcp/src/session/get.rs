//! GET command
// (c) 2024-5 Ross Younger

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;
use tracing::trace;

use crate::Parameters;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendingStream};
use crate::protocol::session::prelude::*;
use crate::protocol::session::{
    FileHeader, FileHeaderV2, FileTrailer, FileTrailerV2, Get2Args, GetArgs,
};
use crate::session::common::FindOption as _;
use crate::session::handler::{CommandHandler, SessionCommandInner};
use crate::session::{CommandStats, RequestResult, error_and_return};

// Extension trait!
use crate::util::FileExt as _;

pub(crate) struct GetHandler;

#[async_trait]
impl CommandHandler for GetHandler {
    type Args = Get2Args;

    async fn send_impl<'a, S: SendingStream, R: ReceivingStream>(
        &mut self,
        inner: &mut SessionCommandInner<'a, S, R>,
        job: &crate::client::CopyJobSpec,
        params: Parameters,
    ) -> Result<RequestResult> {
        let filename = &job.source.filename;
        let dest = &job.destination.filename;

        let real_start = Instant::now();
        let cmd = if inner.compat.supports(Feature::GET2_PUT2) {
            let mut options = vec![];
            if job.preserve {
                options.push(CommandParam::PreserveMetadata.into());
            }
            Command::Get2(Get2Args {
                filename: filename.clone(),
                options,
            })
        } else {
            Command::Get(GetArgs {
                filename: filename.clone(),
            })
        };
        trace!("send command: {cmd}");
        cmd.to_writer_async_framed(&mut inner.stream.send).await?;
        inner.stream.send.flush().await?;

        trace!("await response");
        let _ = Response::from_reader_async_framed(&mut inner.stream.recv)
            .await?
            .into_result()
            .with_context(|| format!("GET {filename} failed"))?;

        let header = FileHeader::from_reader_async_framed(&mut inner.stream.recv).await?;
        trace!("{header:?}");
        let header = FileHeaderV2::from(header);
        let mut file = TokioFile::create_or_truncate(dest, &header).await?;

        // Now we know how much we're receiving, update the chrome.
        // File Trailers are currently 5-17 bytes on the wire; hardly material.

        // Unfortunately, the file data is already well in flight at this point, leading to a flood of packets
        // that causes the estimated rate to spike unhelpfully at the beginning of the transfer.
        // Therefore we incorporate time in flight so far to get the estimate closer to reality.
        let progress_bar = inner
            .ui
            .progress_bar_for(job, header.size.0 + 17, params.quiet)?
            .with_elapsed(Instant::now().duration_since(real_start));

        let mut meter = crate::client::meter::InstaMeterRunner::new(
            &progress_bar,
            Some(inner.spinner().clone()),
            inner.config.rx(),
        );
        meter.start().await;

        let inbound = progress_bar.wrap_async_read(&mut inner.stream.recv);

        let mut inbound = inbound.take(header.size.0);
        trace!("payload");
        let _ = crate::util::io::copy_large(&mut inbound, &mut file, inner.config.io_buffer_size)
            .await?;
        // Retrieve the stream from within the Take wrapper for further operations
        let mut inbound = inbound.into_inner();

        let trailer =
            FileTrailerV2::from(FileTrailer::from_reader_async_framed(&mut inbound).await?);
        // Even if we only get the older V1 trailer, the server believes the file was sent correctly.
        trace!("{trailer:?}");

        // Note that the Quinn send stream automatically calls finish on drop.
        meter.stop().await;
        file.flush().await?;

        file = file.update_metadata(&trailer.metadata).await?;
        drop(file);

        trace!("complete");
        progress_bar.finish_and_clear();
        Ok(RequestResult::new(
            CommandStats {
                payload_bytes: header.size.0,
                peak_transfer_rate: meter.peak(),
            },
            None,
        ))
    }

    async fn handle_impl<'a, S: SendingStream, R: ReceivingStream>(
        &mut self,
        inner: &mut SessionCommandInner<'a, S, R>,
        args: &Get2Args,
    ) -> Result<()> {
        trace!("begin");
        let stream = &mut inner.stream;
        let compat = inner.compat;

        let path = PathBuf::from(&args.filename);

        let (mut file, file_original_meta) = match TokioFile::open_with_meta(&args.filename).await {
            Ok(res) => res,
            Err(e) => error_and_return!(stream, e),
        };
        if file_original_meta.is_dir() {
            error_and_return!(stream, Status::ItIsADirectory);
        }

        // We believe we can fulfil this request.
        trace!("responding OK");
        crate::session::common::send_ok(&mut stream.send).await?;

        let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks

        let hdr = FileHeader::for_file(compat, &file_original_meta, protocol_filename);
        trace!("{hdr:?}");
        hdr.to_writer_async_framed(&mut stream.send).await?;

        trace!("sending file payload");
        let result =
            crate::util::io::copy_large(&mut file, &mut stream.send, inner.config.io_buffer_size)
                .await;
        anyhow::ensure!(result.is_ok(), "copy ended prematurely");
        anyhow::ensure!(
            result.is_ok_and(|r| r == file_original_meta.len()),
            "logic error: file sent size doesn't match metadata"
        );

        let preserve = args
            .options
            .find_option(CommandParam::PreserveMetadata)
            .is_some();

        let trl = FileTrailer::for_file(compat, &file_original_meta, preserve);
        trace!("send trailer {trl:?}");
        trl.to_writer_async_framed(&mut stream.send).await?;

        stream.send.flush().await?;
        trace!("complete");
        Ok(())
    }
}

#[cfg(any(test, feature = "unstable-test-helpers"))]
#[cfg_attr(coverage_nightly, coverage(off))]
/// Test helper functions exposed for qcp-unsafe-tests
pub(crate) mod test_shared {
    use anyhow::{Result, bail};
    use either::Left;

    use crate::{
        Configuration, Parameters,
        client::CopyJobSpec,
        protocol::{
            control::Compatibility,
            session::{Command, CommandParam, Get2Args},
            test_helpers::{new_test_plumbing, read_from_stream},
        },
        session::{
            RequestResult, SessionCommandImpl as _,
            handler::{GetHandler, SessionCommand},
        },
    };

    /// Run a GET to completion, return the results from sender & receiver.
    #[allow(clippy::missing_panics_doc)] // this is a gated test function
    #[allow(unreachable_pub)] // Selectively exported by qcp::test_helpers
    pub async fn test_getx_main(
        file1: &str,
        file2: &str,
        client_level: u16,
        server_level: u16,
        preserve: bool,
    ) -> Result<(Result<RequestResult>, Result<()>)> {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let spec = CopyJobSpec::from_parts(file1, file2, preserve, false).unwrap();
        let mut options = Vec::new();
        if preserve {
            options.push(CommandParam::PreserveMetadata.into());
        }
        let args = Get2Args {
            filename: file1.to_string(),
            options,
        };
        let mut sender = SessionCommand::boxed(
            pipe1,
            GetHandler,
            Some(args),
            Compatibility::Level(client_level),
            Configuration::system_default(),
            None,
        );
        let params = Parameters {
            quiet: true,
            ..Default::default()
        };
        let fut = sender.send(&spec, params);
        tokio::pin!(fut);

        let Left(result) = read_from_stream(&mut pipe2.recv, &mut fut).await else {
            bail!("Get sender should not have bailed")
        };
        let args = match result? {
            Command::Get(aa) => {
                anyhow::ensure!(client_level == 1);
                aa.into()
            }
            Command::Get2(aa) => {
                anyhow::ensure!(client_level > 1);
                aa
            }
            _ => bail!("expected Get or Get2 command"),
        };

        let mut handler = SessionCommand::boxed(
            pipe2,
            GetHandler,
            Some(args),
            Compatibility::Level(server_level),
            Configuration::system_default(),
            None,
        );
        let (r1, r2) = tokio::join!(fut, handler.handle());
        Ok((r1, r2))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    // See also qcp-unsafe-tests/src/unix_umask.rs

    use anyhow::Result;
    use littertray::LitterTray;
    use pretty_assertions::assert_eq;

    use super::test_shared::test_getx_main;
    use crate::{
        Configuration,
        protocol::{control::Compatibility, session::Status, test_helpers::new_test_plumbing},
        session::{
            RequestResult, SessionCommandImpl as _,
            handler::{GetHandler, SessionCommand},
        },
        util::time::SystemTimeExt as _,
    };
    use std::{fs::FileTimes, time::SystemTime};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    async fn test_get_main(
        file1: &str,
        file2: &str,
    ) -> Result<(Result<RequestResult>, Result<()>)> {
        test_getx_main(file1, file2, 2, 2, false).await
    }

    #[cfg_attr(cross_target_mingw, ignore)]
    // TODO: Cross-compiled mingw code fails here in quinn::Endpoint::new
    // with Endpoint Failed: OS Error 10045 (FormatMessageW() returned error 317) (os error 10045)
    // Don't run this test on such cross builds for now.
    #[tokio::test]
    async fn get_happy_path() -> Result<()> {
        let contents = "hello";
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("file1", contents)?;
            let (r1, r2) = test_get_main("s:file1", "file2").await?;
            assert_eq!(r1?.stats.payload_bytes, contents.len() as u64);
            assert!(r2.is_ok());
            let readback = std::fs::read_to_string("file2")?;
            assert_eq!(readback, contents);
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn file_not_found() -> Result<()> {
        LitterTray::try_with_async(async |_tray| {
            let (r1, r2) = test_get_main("s:file1", "file2").await?;
            assert_eq!(Status::from(r1), Status::FileNotFound);
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn is_a_dir() -> Result<()> {
        LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("td")?;
            let (r1, r2) = test_get_main("s:td", "file2").await?;
            let status = Status::from(r1);
            if cfg!(windows) {
                assert_eq!(status, Status::IncorrectPermissions);
            } else {
                assert_eq!(status, Status::ItIsADirectory);
            }
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

    #[cfg(linux)] // TODO: Make more cross-platform
    #[tokio::test]
    async fn permission_denied() -> Result<()> {
        LitterTray::try_with_async(async |_tray| {
            let (r1, r2) = test_get_main("s:/etc/shadow", "file2").await?;
            assert_eq!(Status::from(r1), Status::IncorrectPermissions);
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn logic_error_trap() {
        let (_pipe1, pipe2) = new_test_plumbing();
        let mut cmd = SessionCommand::boxed(
            pipe2,
            GetHandler,
            None,
            Compatibility::Level(1),
            Configuration::system_default(),
            None,
        );

        assert!(cmd.handle().await.is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    // Test for behaviour equivalence to openssh:
    // Copy without the --preserve option should still keep the +x bit.
    async fn get_preserves_execute_bit() {
        use std::fs::{Permissions, metadata, set_permissions};

        let file1 = "file_x";
        let file2 = "file_no_x";
        LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("created")?;
            // Test 1: Without execute bit (current behaviour before fixing #77)
            let _ = tray.create_text(file2, "22")?;
            set_permissions(file2, Permissions::from_mode(0o644))?;

            // Note that we are deliberately NOT setting the --preserve option.
            let (r1, r2) = test_get_main("s:file_no_x", "created/file_no_x").await?;
            let _ = r1.unwrap();
            r2.unwrap();
            let mode = metadata("created/file_no_x")
                .expect("created file should exist")
                .permissions()
                .mode();
            // We're not testing umask here, so only look at owner permissions.
            // See also qcp-unsafe-tests/src/unix_umask.rs
            assert_eq!(mode & 0o700, 0o600); // execute bit should not be set

            // Test 2: With execute bit set (fix for #77)
            let _ = tray.create_text(file1, "11")?;
            set_permissions(file1, Permissions::from_mode(0o755))?;
            let (r1, r2) = test_get_main("s:file_x", "created/file_x").await?;
            let _ = r1.unwrap();
            r2.unwrap();
            let mode = metadata("created/file_x")
                .expect("created file should exist")
                .permissions()
                .mode();
            // We're not testing umask here, so only look at owner permissions.
            assert_eq!(mode & 0o700, 0o700);

            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn get_preserve_atime_mtime() {
        LitterTray::try_with_async(async |tray| {
            let file = tray.create_text("hi", "hi")?;
            let times = FileTimes::new()
                .set_accessed(SystemTime::from_unix(12345))
                .set_modified(SystemTime::from_unix(654_321));
            file.set_times(times)?;
            drop(file);

            let (r1, r2) = test_getx_main("hi", "remote:hi2", 2, 2, true).await?;
            assert!(r1.is_ok());
            assert!(r2.is_ok());

            let meta = std::fs::metadata("hi2")?;
            assert_eq!(meta.modified()?, SystemTime::from_unix(654_321));
            assert_eq!(meta.accessed()?, SystemTime::from_unix(12345));
            Ok(())
        })
        .await
        .unwrap();
    }

    async fn compat_get(client: u16, server: u16, preserve: bool) {
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("aa", "aa")?;
            let (r1, r2) = test_getx_main("srv:aa", "aa2", client, server, preserve).await?;
            assert!(r1.is_ok());
            assert!(r2.is_ok());
            let _meta = std::fs::metadata("aa2")?;

            Ok(())
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn compat_v1_v2() {
        compat_get(1, 2, false).await;
    }
    #[tokio::test]
    async fn compat_v2_v1() {
        compat_get(2, 1, false).await;
    }
    #[tokio::test]
    async fn compat_v1_v1() {
        compat_get(1, 1, false).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn device_nodes_disallowed() {
        // Get from a device node
        LitterTray::try_with_async(async |_| {
            let (r1, r2) = test_getx_main("srv:/dev/null", "file", 2, 2, true).await?;
            assert!(r1.is_err_and(|e| e.root_cause().to_string().contains("not a regular file")));
            assert!(r2.is_ok());
            let _meta_err = std::fs::metadata("created/file").unwrap_err();
            Ok(())
        })
        .await
        .unwrap();

        // Get to a device node
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("file", "hi there")?;
            let (r1, r2) = test_getx_main("srv:file", "/dev/null", 2, 2, true).await?;
            assert!(r1.is_err_and(|e| e.root_cause().to_string().contains("not a regular file")));
            assert!(r2.is_ok());
            let _meta_err = std::fs::metadata("created/file").unwrap_err();
            Ok(())
        })
        .await
        .unwrap();
    }
}
