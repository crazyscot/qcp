//! GET command
// (c) 2024-5 Ross Younger

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;
use tracing::trace;

use super::{CommandStats, SessionCommandImpl};

use crate::Parameters;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::compat::Feature;
use crate::protocol::control::Compatibility;
use crate::protocol::session::{
    Command, CommandParam, FileHeader, FileHeaderV2, FileTrailer, FileTrailerV2, Get2Args, GetArgs,
    Response, Status,
};

use crate::protocol::Variant;
use crate::session::common::{progress_bar_for, send_response};

// Extension trait!
use crate::util::FileExt as _;

pub(crate) struct Get<S: SendingStream, R: ReceivingStream> {
    stream: SendReceivePair<S, R>,
    args: Option<Get2Args>,
    compat: Compatibility, // Selected compatibility level for the command
}

impl<S: SendingStream + 'static, R: ReceivingStream + 'static> Get<S, R> {
    /// Boxing constructor
    pub(crate) fn boxed(
        stream: SendReceivePair<S, R>,
        args: Option<Get2Args>,
        compat: Compatibility,
    ) -> Box<dyn SessionCommandImpl> {
        Box::new(Self {
            stream,
            args,
            compat,
        })
    }
}

impl<S: SendingStream, R: ReceivingStream> Get<S, R> {
    /// Accessor
    pub(crate) fn find_option(&self, opt: CommandParam) -> Option<&Variant> {
        use crate::protocol::FindTag as _;
        self.args.as_ref().and_then(|a| a.options.find_tag(opt))
    }
}

#[async_trait]
impl<S: SendingStream, R: ReceivingStream> SessionCommandImpl for Get<S, R> {
    async fn send(
        &mut self,
        job: &crate::client::CopyJobSpec,
        display: indicatif::MultiProgress,
        filename_width: usize,
        spinner: indicatif::ProgressBar,
        config: &crate::config::Configuration,
        params: Parameters,
    ) -> Result<CommandStats> {
        let filename = &job.source.filename;
        let dest = &job.destination.filename;

        let real_start = Instant::now();
        let cmd = if self.compat.supports(Feature::GET2_PUT2) {
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
        cmd.to_writer_async_framed(&mut self.stream.send).await?;
        self.stream.send.flush().await?;

        trace!("await response");
        let _ = Response::from_reader_async_framed(&mut self.stream.recv)
            .await?
            .into_result()
            .with_context(|| format!("GET {filename} failed"))?;

        let header = FileHeader::from_reader_async_framed(&mut self.stream.recv).await?;
        trace!("{header:?}");
        let header = FileHeaderV2::from(header);
        let mut file = TokioFile::create_or_truncate(dest, &header).await?;

        // Now we know how much we're receiving, update the chrome.
        // File Trailers are currently 5-17 bytes on the wire; hardly material.

        // Unfortunately, the file data is already well in flight at this point, leading to a flood of packets
        // that causes the estimated rate to spike unhelpfully at the beginning of the transfer.
        // Therefore we incorporate time in flight so far to get the estimate closer to reality.
        let progress_bar = progress_bar_for(
            &display,
            job,
            filename_width,
            header.size.0 + 17,
            params.quiet,
        )?
        .with_elapsed(Instant::now().duration_since(real_start));

        let mut meter =
            crate::client::meter::InstaMeterRunner::new(&progress_bar, spinner, config.rx());
        meter.start().await;

        let inbound = progress_bar.wrap_async_read(&mut self.stream.recv);

        let mut inbound = inbound.take(header.size.0);
        trace!("payload");
        let _ = crate::util::io::copy_large(&mut inbound, &mut file, config.io_buffer_size).await?;
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
        Ok(CommandStats {
            payload_bytes: header.size.0,
            peak_transfer_rate: meter.peak(),
        })
    }

    async fn handle(&mut self, io_buffer_size: u64) -> Result<()> {
        let Some(ref args) = self.args else {
            anyhow::bail!("GET handler called without args");
        };
        trace!("begin");

        let path = PathBuf::from(&args.filename);

        let (mut file, file_original_meta) = match TokioFile::open_with_meta(&args.filename).await {
            Ok(res) => res,
            Err(e) => {
                let (status, message) = crate::util::io::status_from_error(&e);
                return send_response(&mut self.stream.send, status, message.as_deref()).await;
            }
        };
        if file_original_meta.is_dir() {
            return send_response(&mut self.stream.send, Status::ItIsADirectory, None).await;
        }

        // We believe we can fulfil this request.
        trace!("responding OK");
        send_response(&mut self.stream.send, Status::Ok, None).await?;

        let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks

        let hdr = FileHeader::for_file(self.compat, &file_original_meta, protocol_filename);
        trace!("{hdr:?}");
        hdr.to_writer_async_framed(&mut self.stream.send).await?;

        trace!("sending file payload");
        let result =
            crate::util::io::copy_large(&mut file, &mut self.stream.send, io_buffer_size).await;
        anyhow::ensure!(result.is_ok(), "copy ended prematurely");
        anyhow::ensure!(
            result.is_ok_and(|r| r == file_original_meta.len()),
            "logic error: file sent size doesn't match metadata"
        );

        let trl = FileTrailer::for_file(
            self.compat,
            &file_original_meta,
            self.find_option(CommandParam::PreserveMetadata).is_some(),
        );
        trace!("send trailer {trl:?}");
        trl.to_writer_async_framed(&mut self.stream.send).await?;

        self.stream.send.flush().await?;
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
        session::{CommandStats, Get},
        util::io::DEFAULT_COPY_BUFFER_SIZE,
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
    ) -> Result<(Result<CommandStats>, Result<()>)> {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let spec = CopyJobSpec::from_parts(file1, file2, preserve).unwrap();
        let mut options = Vec::new();
        if preserve {
            options.push(CommandParam::PreserveMetadata.into());
        }
        let args = Get2Args {
            filename: file1.to_string(),
            options,
        };
        let mut sender = Get::boxed(pipe1, Some(args), Compatibility::Level(client_level));
        let params = Parameters {
            quiet: true,
            ..Default::default()
        };
        let fut = sender.send(
            &spec,
            indicatif::MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden()),
            10,
            indicatif::ProgressBar::hidden(),
            Configuration::system_default(),
            params,
        );
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

        let mut handler = Get::boxed(pipe2, Some(args), Compatibility::Level(server_level));
        let (r1, r2) = tokio::join!(fut, handler.handle(DEFAULT_COPY_BUFFER_SIZE));
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
        protocol::test_helpers::new_test_plumbing,
        protocol::{control::Compatibility, session::Status},
        session::{CommandStats, Get},
        util::io::DEFAULT_COPY_BUFFER_SIZE,
        util::time::SystemTimeExt as _,
    };
    use std::{fs::FileTimes, time::SystemTime};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    async fn test_get_main(file1: &str, file2: &str) -> Result<(Result<CommandStats>, Result<()>)> {
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
            assert_eq!(r1?.payload_bytes, contents.len() as u64);
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
        assert!(
            Get::boxed(pipe2, None, Compatibility::Level(1))
                .handle(DEFAULT_COPY_BUFFER_SIZE)
                .await
                .is_err()
        );
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
