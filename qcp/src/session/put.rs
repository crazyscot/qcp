//! PUT command
// (c) 2024-5 Ross Younger

use anyhow::{Context as _, Result};
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::fs::File as TokioFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{debug, error, trace};

use super::{CommandStats, SessionCommandImpl, error_and_return};

use crate::Parameters;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::compat::Feature;
use crate::protocol::control::Compatibility;
use crate::protocol::session::{
    Command, CommandParam, FileHeader, FileHeaderV2, FileTrailer, FileTrailerV2, Put2Args, PutArgs,
    Response, Status,
};
use crate::session::common::progress_bar_for;

// Extension trait for TokioFile!
use crate::util::FileExt as _;

pub(crate) struct Put<S: SendingStream, R: ReceivingStream> {
    stream: SendReceivePair<S, R>,
    args: Option<Put2Args>,
    compat: Compatibility, // Selected compatibility level for the command
}

/// Boxing constructor
impl<S: SendingStream + 'static, R: ReceivingStream + 'static> Put<S, R> {
    pub(crate) fn boxed(
        stream: SendReceivePair<S, R>,
        args: Option<Put2Args>,
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
impl<S: SendingStream, R: ReceivingStream> SessionCommandImpl for Put<S, R> {
    async fn send(
        &mut self,
        job: &crate::client::CopyJobSpec,
        display: indicatif::MultiProgress,
        filename_width: usize,
        spinner: indicatif::ProgressBar,
        config: &crate::config::Configuration,
        params: Parameters,
    ) -> Result<CommandStats> {
        let src_filename = &job.source.filename;
        let dest_filename = &job.destination.filename;

        let path = PathBuf::from(src_filename);
        let (mut file, src_meta) = TokioFile::open_with_meta(src_filename).await?;
        if src_meta.is_dir() {
            anyhow::bail!("PUT: Source is a directory");
        }

        let payload_len = src_meta.len();

        // Now we can compute how much we're going to send, update the chrome.
        // Marshalled commands are currently 48 bytes + filename length
        // File headers are currently 36 + filename length; Trailers are 16 bytes.
        let steps = payload_len + 48 + 36 + 16 + 2 * dest_filename.len() as u64;
        let progress_bar = progress_bar_for(&display, job, filename_width, steps, params.quiet)?;
        let mut outbound = progress_bar.wrap_async_write(&mut self.stream.send);
        let mut meter =
            crate::client::meter::InstaMeterRunner::new(&progress_bar, spinner, config.tx());
        meter.start().await;

        trace!("sending command");

        let cmd = if self.compat.supports(Feature::GET2_PUT2) {
            let mut options = vec![];
            if job.preserve {
                options.push(CommandParam::PreserveMetadata.into());
            }
            Command::Put2(Put2Args {
                filename: dest_filename.clone(),
                options,
            })
        } else {
            Command::Put(PutArgs {
                filename: dest_filename.clone(),
            })
        };
        cmd.to_writer_async_framed(&mut outbound).await?;
        outbound.flush().await?;

        // The filename in the protocol is the file part only of src_filename
        trace!("send header");
        let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks
        let hdr = FileHeader::for_file(self.compat, &src_meta, protocol_filename);
        trace!("{hdr:?}");
        hdr.to_writer_async_framed(&mut outbound).await?;

        trace!("await response");
        let _ = Response::from_reader_async_framed(&mut self.stream.recv)
            .await?
            .into_result()
            .with_context(|| format!("PUTx {src_filename} failed"))?;

        // A server-side abort might happen part-way through a large transfer.
        trace!("send payload");
        let result =
            crate::util::io::copy_large(&mut file, &mut outbound, config.io_buffer_size).await;

        match result {
            Ok(sent) if sent == src_meta.len() => (),
            Ok(sent) => {
                anyhow::bail!(
                    "File sent size {sent} doesn't match its metadata {}",
                    src_meta.len()
                );
            }
            Err(e) => {
                if e.kind() == tokio::io::ErrorKind::ConnectionReset {
                    // Maybe the connection was cut, maybe the server sent something to help us inform the user.
                    let Ok(response) =
                        Response::from_reader_async_framed(&mut self.stream.recv).await
                    else {
                        anyhow::bail!("connection closed unexpectedly");
                    };
                    let Response::V1(response) = response;
                    anyhow::bail!(
                        "remote closed connection: {:?}: {}",
                        response.status,
                        response.message.unwrap_or("(no message)".into())
                    );
                }
                anyhow::bail!(
                    "Unknown I/O error during PUT: {e}/{:?}/{:?}",
                    e.kind(),
                    e.raw_os_error()
                );
            }
        }

        let trl = FileTrailer::for_file(self.compat, &src_meta, job.preserve);
        trace!("send trailer {trl:?}");
        trl.to_writer_async_framed(&mut outbound).await?;
        outbound.flush().await?;
        meter.stop().await;

        let response = Response::from_reader_async_framed(&mut self.stream.recv).await?;
        #[allow(irrefutable_let_patterns)]
        let Response::V1(response) = response else {
            todo!()
        };
        if response.status != Status::Ok {
            anyhow::bail!(format!(
                "PUTx ({src_filename}) failed on completion check: {response}"
            ));
        }

        // Note that the Quinn sendstream calls finish() on drop.
        trace!("complete");
        progress_bar.finish_and_clear();
        Ok(CommandStats {
            payload_bytes: payload_len,
            peak_transfer_rate: meter.peak(),
        })
    }

    async fn handle(&mut self, io_buffer_size: u64) -> Result<()> {
        let Some(ref args) = self.args else {
            anyhow::bail!("PUT handler called without args");
        };
        let destination = &args.filename;
        trace!("begin");

        // Initial checks. Is the destination valid, do we need to append the filename (from the `FileHeader`) to the destination path?
        // This is moderately tricky. It might validly be empty, a directory, a file, it might be a nonexistent file in an extant directory.
        let mut path = PathBuf::from(destination.clone());
        let append_filename = if destination.is_empty() || destination == "." {
            // Easy case: copying to current working directory
            true
        } else if path.is_dir() || path.is_file() {
            // The destination exists. This is another easy case; append filename only if it is a directory.
            path.is_dir()
        } else {
            // The given destination does not exist. The possible cases here are:
            // - The destination is clearly intended as a directory (ends with / or \).
            //   This is an error (there's a separate CreateDirectory command for that).
            if destination.ends_with(std::path::MAIN_SEPARATOR) {
                // N.B. Path.has_trailing_sep() is currently only available in nightly
                debug!("Nonexistent destination directory {destination}");
                error_and_return!(self, Status::DirectoryDoesNotExist);
            }

            // - The destination's parent directory exists => do not append the path
            // - The destination's parent directory does not exist => error

            let mut parent_dir = {
                let mut tmp = path.clone();
                let _ = tmp.pop();
                tmp
            };

            if parent_dir.as_os_str().is_empty() {
                // We're writing a file to the current working directory, so apply the is_dir check
                parent_dir.push(".");
            }
            if parent_dir.is_dir() {
                false // destination path is fully specified, do not append filename
            } else {
                error_and_return!(self, Status::DirectoryDoesNotExist);
            }
        };

        let header = FileHeader::from_reader_async_framed(&mut self.stream.recv).await?;
        trace!("{header:?}");
        let header = FileHeaderV2::from(header);

        debug!("PUT {} -> {destination}", &header.filename);
        if append_filename {
            path.push(&header.filename);
        }
        let mut file = match TokioFile::create_or_truncate(path, &header).await {
            Ok(f) => f,
            Err(e) => {
                let str = e.to_string();
                debug!("Could not write to destination: {str}");
                error_and_return!(self, e);
            }
        };

        // So far as we can tell, we believe we will be able to fulfil this request.
        // We might still fail with an I/O error.
        trace!("responding OK");
        crate::session::common::send_ok(&mut self.stream.send).await?;
        self.stream.send.flush().await?;

        trace!("receiving file payload");
        let result = limited_copy(
            &mut self.stream.recv,
            header.size.0,
            &mut file,
            io_buffer_size,
        )
        .await;
        if let Err(e) = result {
            error!("Failed to write to destination: {e}");
            error_and_return!(self, e);
        }

        trace!("receiving trailer");
        let trailer = FileTrailerV2::from(
            FileTrailer::from_reader_async_framed(&mut self.stream.recv).await?,
        );
        // Even if we only get the older V1 trailer, the server believes the file was sent correctly.
        trace!("{trailer:?}");

        file.flush().await?;
        file = file.update_metadata(&trailer.metadata).await?;
        drop(file);

        crate::session::common::send_ok(&mut self.stream.send).await?;
        self.stream.send.flush().await?;
        trace!("complete");
        Ok(())
    }
}

// this function exists because without it, the compiler complains that we're _moving_
// Put's self.stream.recv; but _borrowing_ it and consuming it is OK.
// This doesn't seem wholly comfortable, but it works.
async fn limited_copy(
    recv: &mut dyn ReceivingStream,
    n: u64,
    f: &mut TokioFile,
    buffer_size: u64,
) -> Result<u64, std::io::Error> {
    let mut limited = recv.take(n);
    crate::util::io::copy_large(&mut limited, f, buffer_size).await
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use std::{fs::FileTimes, time::SystemTime};

    use anyhow::{Result, bail};
    use assertables::assert_contains;
    use pretty_assertions::assert_eq;

    use crate::{
        Configuration, Parameters,
        client::CopyJobSpec,
        protocol::{
            control::Compatibility,
            session::{Command, Status},
            test_helpers::{new_test_plumbing, read_from_stream},
        },
        session::{CommandStats, Put},
        util::{io::DEFAULT_COPY_BUFFER_SIZE, time::SystemTimeExt as _},
    };
    use littertray::LitterTray;

    /// Run a PUT (with the ability to send the Preserve option), return the results from sender & receiver.
    ///
    /// If `sender_bails`, we assert that the sender reports an error before outputting its first Command.
    /// In this case we return (Sender's result, Ok(())).
    ///
    /// Otherwise, we wait for both to complete and return a composite result (Sender's result, Handler's result).
    async fn test_put_main(
        file1: &str,
        file2: &str,
        sender_bails: bool,
    ) -> Result<(Result<CommandStats>, Result<()>)> {
        test_putx_main(file1, file2, 2, 2, sender_bails, false).await
    }

    /// Run a PUT, return the results from sender & receiver.
    ///
    /// If `sender_bails`, we assert that the sender reports an error before outputting its first Command.
    /// In this case we return (Sender's result, Ok(())).
    ///
    /// Otherwise, we wait for both to complete and return a composite result (Sender's result, Handler's result).
    async fn test_putx_main(
        file1: &str,
        file2: &str,
        client_level: u16,
        server_level: u16,
        sender_bails: bool,
        preserve: bool,
    ) -> Result<(Result<CommandStats>, Result<()>)> {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let spec = CopyJobSpec::from_parts(file1, file2, preserve, false).unwrap();
        let mut sender = Put::boxed(pipe1, None, Compatibility::Level(client_level));
        let params = Parameters {
            quiet: true,
            ..Default::default()
        };
        let sender_fut = sender.send(
            &spec,
            indicatif::MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden()),
            10,
            indicatif::ProgressBar::hidden(),
            Configuration::system_default(),
            params,
        );
        tokio::pin!(sender_fut);

        // The first difference between Get and Put is that in the error cases for Put, the sending future
        // might finish quickly with an error.
        // (Put sender does not currently return error codes. One day...)
        let result = read_from_stream(&mut pipe2.recv, &mut sender_fut).await;
        if sender_bails {
            let e = result.expect_right("sender should have completed early");
            anyhow::ensure!(e.is_err(), "sender should have bailed");
            return Ok((e, Ok(())));
        }
        let cmd = result.expect_left("sender should not have completed early")?;
        let args = match cmd {
            Command::Put(aa) => {
                anyhow::ensure!(client_level == 1);
                aa.into()
            }
            Command::Put2(aa) => {
                anyhow::ensure!(client_level > 1);
                aa
            }
            _ => bail!("expected Put or Put2 command"),
        };

        // The second difference is that the receiver might send a failure response and shut down the stream.
        // This isn't well simulated by our test pipe.
        let mut handler = Put::boxed(pipe2, Some(args), Compatibility::Level(server_level));
        let (r1, r2) = tokio::join!(sender_fut, handler.handle(DEFAULT_COPY_BUFFER_SIZE));
        Ok((r1, r2))
    }

    #[tokio::test]
    async fn put_success() -> Result<()> {
        let contents = "wibble";
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("file1", contents)?;
            let (r1, r2) = test_put_main("file1", "s:file2", false).await?;
            assert_eq!(r1?.payload_bytes, contents.len() as u64);
            assert!(r2.is_ok());
            let readback = std::fs::read_to_string("file2")?;
            assert_eq!(readback, contents);
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn put_to_login_dir() -> Result<()> {
        let contents = "wibble";
        LitterTray::try_with_async(async |tray| {
            // `qcp file server:`
            // Sender and receiver share the same litter tray, so we have to send from a another directory to make it testable.
            let _ = tray.make_dir("send_dir")?;
            let _ = tray.create_text("send_dir/file1", contents)?;
            assert!(!std::fs::exists("file1")?); // ensure the test is valid
            let (r1, r2) = test_put_main("send_dir/file1", "s:", false).await?;
            assert_eq!(r1?.payload_bytes, contents.len() as u64);
            assert!(r2.is_ok());
            let readback = std::fs::read_to_string("file1")?;
            assert_eq!(readback, contents);
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn source_file_not_found() -> Result<()> {
        LitterTray::try_with_async(async |_tray| {
            let (r1, r2) = test_put_main("file1", "s:file2", true).await?;
            let msg = r1.unwrap_err().to_string();
            if cfg!(unix) {
                assert_contains!(msg, "No such file or directory");
            } else if cfg!(msvc) {
                assert_contains!(msg, "The system cannot find the file specified");
            } else {
                // mingw
                assert_contains!(msg, "File not found");
            }
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn source_is_a_directory() -> Result<()> {
        LitterTray::try_with_async(async |_tray| {
            let (r1, r2) = test_put_main("/tmp", "s:foo", true).await?;
            let msg = r1.unwrap_err().to_string();
            if cfg!(unix) {
                assert_contains!(msg, "Source is a directory");
            } else if cfg!(msvc) {
                assert_contains!(msg, "The specified path is invalid");
            } else {
                // mingw
                assert_contains!(msg, "Access denied");
            }
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn write_to_directory() -> Result<()> {
        let contents = "teapot";
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("file1", contents)?;
            let _ = tray.make_dir("destdir")?;
            let (r1, r2) = test_put_main("file1", "s:destdir", false).await?;
            assert_eq!(r1?.payload_bytes, contents.len() as u64);
            assert!(r2.is_ok());
            let readback = std::fs::read_to_string("destdir/file1")?;
            assert_eq!(readback, contents);
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn write_fail_parent_directory_missing() -> Result<()> {
        let contents = "xyzy";
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("file1", contents)?;
            let (r1, r2) = test_put_main("file1", "s:destdir/foo", false).await?;
            let r1 = r1.unwrap_err();
            assert_eq!(Status::from(r1), Status::DirectoryDoesNotExist);
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn write_fail_dest_dir_missing() -> Result<()> {
        let contents = "foo";
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("file1", contents)?;
            let (r1, r2) = test_put_main("file1", "s:destdir/", false).await?;
            let r1 = r1.unwrap_err();
            let status = Status::from(r1);
            if cfg!(windows) {
                assert_eq!(status, Status::IoError);
            } else {
                assert_eq!(status, Status::DirectoryDoesNotExist);
            }
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn write_fail_permissions() -> Result<()> {
        let contents = "xvcoffee";
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("file1", contents)?;
            let (r1, r2) = test_put_main("file1", "s:/dev/", false).await?;
            let r1 = r1.unwrap_err();
            if cfg!(msvc) {
                assert_eq!(Status::from(r1), Status::DirectoryDoesNotExist);
            } else {
                assert_eq!(Status::from(r1), Status::IncorrectPermissions);
            }
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

    #[tokio::test]
    async fn logic_error_trap() {
        let (_pipe1, pipe2) = new_test_plumbing();
        assert!(
            Put::boxed(pipe2, None, Compatibility::Level(2))
                .handle(DEFAULT_COPY_BUFFER_SIZE)
                .await
                .is_err()
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn put_preserves_execute_bit() {
        use std::fs::{Permissions, metadata, set_permissions};
        use std::os::unix::fs::PermissionsExt as _;

        let file1 = "file_x";
        let file2 = "file_no_x";
        LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("created")?;
            // Test 1: Without execute bit (current behaviour before fixing #77)
            let _ = tray.create_text(file2, "22")?;
            set_permissions(file2, Permissions::from_mode(0o644))?;

            let (r1, r2) = test_put_main(file2, "s:created/file_no_x", false).await?;
            let _ = r1.unwrap();
            r2.unwrap();
            let mode = metadata("created/file_no_x")
                .expect("created file should exist")
                .permissions()
                .mode();
            // We're not testing umask here, so only look at owner permissions.
            assert_eq!(mode & 0o700, 0o600); // execute bit should not be set

            // Test 2: With execute bit set (fix for #77)
            let _ = tray.create_text(file1, "11")?;
            set_permissions(file1, Permissions::from_mode(0o755))?;
            let (r1, r2) = test_put_main(file1, "remote:created/file_x", false).await?;
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
    async fn put_preserve_atime_mtime() {
        LitterTray::try_with_async(async |tray| {
            let file = tray.create_text("hi", "hi")?;
            let times = FileTimes::new()
                .set_accessed(SystemTime::from_unix(12345))
                .set_modified(SystemTime::from_unix(654_321));
            file.set_times(times)?;
            drop(file);

            let (r1, r2) = test_putx_main("hi", "remote:hi2", 2, 2, false, true).await?;
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

    async fn compat_put(client: u16, server: u16, preserve: bool) {
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("aa", "aa")?;
            let (r1, r2) = test_putx_main("aa", "srv:aa2", client, server, false, preserve).await?;
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
        compat_put(1, 2, false).await;
    }
    #[tokio::test]
    async fn compat_v2_v1() {
        compat_put(2, 1, false).await;
    }
    #[tokio::test]
    async fn compat_v1_v1() {
        compat_put(1, 1, false).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn device_nodes_disallowed() {
        // Put to a device node
        LitterTray::try_with_async(async |tray| {
            let _ = tray.create_text("file", "aaaa")?;
            let (r1, r2) = test_putx_main("file", "srv:/dev/null", 2, 2, false, false).await?;
            assert!(r1.is_err_and(|e| e.root_cause().to_string().contains("not a regular file")));
            assert!(r2.is_ok());
            Ok(())
        })
        .await
        .unwrap();

        // Put from a device node
        LitterTray::try_with_async(async |_| {
            let (r1, r2) = test_putx_main("/dev/null", "srv:file", 2, 2, true, false).await?;
            assert!(r1.is_err_and(|e| e.root_cause().to_string().contains("not a regular file")));
            assert!(r2.is_ok());
            Ok(())
        })
        .await
        .unwrap();
    }
}
