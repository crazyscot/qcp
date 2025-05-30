//! GET command
// (c) 2024-5 Ross Younger

use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::Instant;
use tracing::trace;

use super::{CommandStats, SessionCommandImpl};

use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::session::{Command, FileHeader, FileTrailer, GetArgs, Response, Status};
use crate::session::common::{check_response, progress_bar_for, send_response};
use crate::util::io::open_file;

pub(crate) struct Get<S: SendingStream, R: ReceivingStream> {
    stream: SendReceivePair<S, R>,
    args: Option<GetArgs>,
}

/// Boxing constructor
impl<S: SendingStream + 'static, R: ReceivingStream + 'static> Get<S, R> {
    pub(crate) fn boxed(
        stream: SendReceivePair<S, R>,
        args: Option<GetArgs>,
    ) -> Box<dyn SessionCommandImpl> {
        Box::new(Self { stream, args })
    }
}

#[async_trait]
impl<S: SendingStream, R: ReceivingStream> SessionCommandImpl for Get<S, R> {
    async fn send(
        &mut self,
        job: &crate::client::CopyJobSpec,
        display: indicatif::MultiProgress,
        spinner: indicatif::ProgressBar,
        config: &crate::config::Configuration,
        quiet: bool,
    ) -> Result<CommandStats> {
        let filename = &job.source.filename;
        let dest = &job.destination.filename;

        let real_start = Instant::now();
        trace!("send command");
        Command::Get(GetArgs {
            filename: filename.to_string(),
        })
        .to_writer_async_framed(&mut self.stream.send)
        .await?;
        self.stream.send.flush().await?;

        // TODO protocol timeout?
        trace!("await response");
        let response = Response::from_reader_async_framed(&mut self.stream.recv).await?;
        let Response::V1(response) = response;
        check_response(response, || format!("GET ({filename})"))?;

        let header = FileHeader::from_reader_async_framed(&mut self.stream.recv).await?;
        trace!("{header:?}");
        let FileHeader::V1(header) = header;

        let mut file = crate::util::io::create_truncate_file(dest, &header).await?;

        // Now we know how much we're receiving, update the chrome.
        // File Trailers are currently 16 bytes on the wire.

        // Unfortunately, the file data is already well in flight at this point, leading to a flood of packets
        // that causes the estimated rate to spike unhelpfully at the beginning of the transfer.
        // Therefore we incorporate time in flight so far to get the estimate closer to reality.
        let progress_bar = progress_bar_for(&display, job, header.size.0 + 16, quiet)?
            .with_elapsed(Instant::now().duration_since(real_start));

        let mut meter =
            crate::client::meter::InstaMeterRunner::new(&progress_bar, spinner, config.rx());
        meter.start().await;

        let inbound = progress_bar.wrap_async_read(&mut self.stream.recv);

        let mut inbound = inbound.take(header.size.0);
        trace!("payload");
        let _ = tokio::io::copy(&mut inbound, &mut file).await?;
        // Retrieve the stream from within the Take wrapper for further operations
        let mut inbound = inbound.into_inner();

        trace!("trailer");
        let _trailer = FileTrailer::from_reader_async_framed(&mut inbound).await?;
        // Trailer is empty for now, but its existence means the server believes the file was sent correctly

        // Note that the Quinn send stream automatically calls finish on drop.
        meter.stop().await;
        file.flush().await?;
        trace!("complete");
        progress_bar.finish_and_clear();
        Ok(CommandStats {
            payload_bytes: header.size.0,
            peak_transfer_rate: meter.peak(),
        })
    }

    async fn handle(&mut self) -> Result<()> {
        let Some(ref args) = self.args else {
            anyhow::bail!("GET handler called without args");
        };
        trace!("begin");

        let path = PathBuf::from(&args.filename);
        let (mut file, meta) = match open_file(&args.filename).await {
            Ok(res) => res,
            Err(e) => {
                let (status, message) = crate::util::io::status_from_error(&e);
                return send_response(&mut self.stream.send, status, Some(&message)).await;
            }
        };
        if meta.is_dir() {
            return send_response(&mut self.stream.send, Status::ItIsADirectory, None).await;
        }

        // We believe we can fulfil this request.
        trace!("responding OK");
        send_response(&mut self.stream.send, Status::Ok, None).await?;

        let protocol_filename = path.file_name().unwrap().to_str().unwrap(); // can't fail with the preceding checks

        FileHeader::new_v1(meta.len(), protocol_filename)
            .to_writer_async_framed(&mut self.stream.send)
            .await?;

        trace!("sending file payload");
        let result = tokio::io::copy(&mut file, &mut self.stream.send).await;
        anyhow::ensure!(
            result.is_ok_and(|r| r == meta.len()),
            "logic error: file sent size doesn't match metadata"
        );

        trace!("sending trailer");
        FileTrailer::V1
            .to_writer_async_framed(&mut self.stream.send)
            .await?;
        self.stream.send.flush().await?;
        trace!("complete");
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use anyhow::{Result, bail};
    use pretty_assertions::assert_eq;

    use crate::{
        client::CopyJobSpec,
        protocol::session::{Command, Status},
        session::{CommandStats, Get, test::*},
        util::test_protocol::test_plumbing,
    };
    use either::Left;
    use littertray::LitterTray;

    /// Run a GET to completion, return the results from sender & receiver.
    async fn test_get_main(file1: &str, file2: &str) -> Result<(Result<CommandStats>, Result<()>)> {
        let (pipe1, mut pipe2) = test_plumbing();
        let spec = CopyJobSpec::from_parts(file1, file2).unwrap();
        let mut sender = Get::boxed(pipe1, None);
        let mut fut = sender.send_test(&spec, None);

        let Left(result) = read_from_plumbing(&mut pipe2.recv, &mut fut).await else {
            bail!("Get sender should not have bailed")
        };
        let Command::Get(args) = result? else {
            bail!("expected Get command");
        };
        let mut handler = Get::boxed(pipe2, Some(args));
        let (r1, r2) = tokio::join!(fut, handler.handle());
        Ok((r1, r2))
    }

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
            assert_eq!(Status::from(r1), Status::ItIsADirectory);
            assert!(r2.is_ok());
            Ok(())
        })
        .await
    }

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
        let (_pipe1, pipe2) = test_plumbing();
        assert!(Get::boxed(pipe2, None).handle().await.is_err());
    }
}
