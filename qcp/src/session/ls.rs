//! List Contents command (remote directory listing)
// (c) 2025 Ross Younger

use anyhow::Result;
use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, trace};
use walkdir::WalkDir;

use super::SessionCommandImpl;

use crate::Parameters;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendReceivePair, SendingStream};
use crate::protocol::session::prelude::*;
use crate::protocol::session::{ListArgs, ListEntry, ListResponse};
use crate::session::CommandStats;
use crate::session::common::{error_to_status, io_error_to_status};

pub(crate) struct Listing<S: SendingStream, R: ReceivingStream> {
    stream: SendReceivePair<S, R>,
    args: Option<ListArgs>,
    compat: Compatibility, // Selected compatibility level for the command
}

/// Boxing constructor
impl<S: SendingStream + 'static, R: ReceivingStream + 'static> Listing<S, R> {
    pub(crate) fn boxed(
        stream: SendReceivePair<S, R>,
        args: Option<ListArgs>,
        compat: Compatibility,
    ) -> Box<dyn SessionCommandImpl> {
        Box::new(Self {
            stream,
            args,
            compat,
        })
    }
}

impl<S: SendingStream, R: ReceivingStream> Listing<S, R> {
    /// Accessor
    pub(crate) fn find_option(&self, opt: CommandParam) -> Option<&Variant> {
        use crate::protocol::FindTag as _;
        self.args.as_ref().and_then(|a| a.options.find_tag(opt))
    }
}

#[async_trait]
impl<S: SendingStream, R: ReceivingStream> SessionCommandImpl for Listing<S, R> {
    async fn send(
        &mut self,
        job: &crate::client::CopyJobSpec,
        _display: indicatif::MultiProgress,
        _filename_width: usize,
        _spinner: indicatif::ProgressBar,
        _config: &crate::config::Configuration,
        params: Parameters,
    ) -> Result<RequestResult> {
        anyhow::ensure!(
            self.compat.supports(Feature::MKDIR_SETMETA_LS),
            "Operation not supported by remote"
        );
        let path = &job.source.filename; // yes, source filename

        // This is a trivial operation, we do not bother with a progress bar.
        trace!("sending command");
        let mut outbound = &mut self.stream.send;
        let mut options = vec![];
        if params.recurse {
            options.push(CommandParam::Recurse.into());
        }
        let cmd = Command::List(ListArgs {
            path: path.clone(),
            options,
        });
        cmd.to_writer_async_framed(&mut outbound).await?;
        outbound.flush().await?;

        trace!("await response");
        let result = Response::from_reader_async_framed(&mut self.stream.recv).await?;
        trace!("result: {:?}", result);
        Ok(RequestResult::new(CommandStats::default(), Some(result)))
    }

    async fn handle(&mut self, _io_buffer_size: u64) -> Result<()> {
        let Some(ref args) = self.args else {
            anyhow::bail!("List handler called without args");
        };
        let path = &args.path;
        let recurse = self.find_option(CommandParam::Recurse).is_some();
        // debug!("ls: path {path}, recurse={recurse}");

        let res = tokio::fs::metadata(path).await;
        let meta = match res {
            Ok(meta) => meta,
            Err(e) => {
                let (st, msg) = io_error_to_status(&e);
                return Response::List(ListResponse {
                    status: st.into(),
                    message: msg,
                    entries: vec![],
                })
                .to_writer_async_framed(&mut self.stream.send)
                .await;
            }
        };
        if meta.is_file() {
            return Response::List(ListResponse {
                status: Status::Ok.into(),
                message: None,
                entries: vec![ListEntry {
                    name: path.clone(),
                    directory: false,
                    size: Uint(meta.len()),
                    attributes: vec![],
                }],
            })
            .to_writer_async_framed(&mut self.stream.send)
            .await;
        }
        let entries: Result<Vec<_>, walkdir::Error> = WalkDir::new(path)
            // do NOT omit the root here, recursive transfer depends on it to mkdir the top-level dir
            .max_depth(if recurse { usize::MAX } else { 1 })
            .follow_links(true)
            .into_iter()
            .map(|e| e.map(ListEntry::from))
            .collect();

        let lcr = match entries {
            Ok(v) => ListResponse {
                status: Status::Ok.into(),
                message: None,
                entries: v,
            },
            Err(e) => {
                debug!("ls: walkdir error: {e}");
                let io = std::io::Error::from(e);
                let (st, msg) = error_to_status(&io.into());
                ListResponse {
                    status: st.into(),
                    message: msg,
                    entries: vec![],
                }
            }
        };
        // debug!("ls: sending response {}", lcr);

        // Careful! The response might be too long for a Response packet (64k).
        if let Err(e) = Response::List(lcr)
            .to_writer_async_framed(&mut self.stream.send)
            .await
        {
            // if we failed to encode: send an error instead.
            error!("Failed to send response: {e}");
            let resp = Response::List(ListResponse {
                status: Status::EncodingFailed.into(),
                message: Some(e.to_string()),
                entries: vec![],
            });
            resp.to_writer_async_framed(&mut self.stream.send).await?;
        }
        self.stream.send.flush().await?;
        trace!("complete");
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use std::collections::HashSet;
    use std::path::MAIN_SEPARATOR;

    use crate::protocol::session::{ListResponse, prelude::*};
    use crate::util::io::DEFAULT_COPY_BUFFER_SIZE;
    use crate::{
        Configuration, Parameters,
        client::CopyJobSpec,
        protocol::test_helpers::{new_test_plumbing, read_from_stream},
        session::Listing,
    };
    use anyhow::{Result, bail};
    use littertray::LitterTray;
    use pretty_assertions::assert_eq;

    async fn test_ls_main(path: &str, recurse: bool) -> Result<ListResponse> {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let mut sender = Listing::boxed(pipe1, None, Compatibility::Level(4));
        let spec =
            CopyJobSpec::from_parts(path, &format!("desthost:{path}"), false, false).unwrap();
        let params = Parameters {
            recurse,
            ..Default::default()
        };
        let result = {
            // this subscope forces sender_fut to unborrow sender.
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
            let Command::List(args) = cmd else {
                bail!("expected CreateDirectory command");
            };

            let mut handler = Listing::boxed(pipe2, Some(args), Compatibility::Level(4));
            let (r1, r2) = tokio::join!(sender_fut, handler.handle(DEFAULT_COPY_BUFFER_SIZE));
            let result = r1.expect("sender should not have failed");
            r2.expect("handler should not have failed");
            result
        };
        let Some(Response::List(lcr)) = result.response else {
            anyhow::bail!("remote sent unexpected List response: {result:?}");
        };
        Ok(lcr)
    }

    // Check for expected results, allowing for walkdir and libc variations.
    fn expected_result(lcr: ListResponse, dir_prefix: &str, expected: &[&str]) {
        assert_eq!(
            Status::try_from(lcr.status).unwrap(),
            Status::Ok,
            "ls failed with status {:?}",
            Status::to_string(lcr.status)
        );
        assert!(lcr.message.is_none());

        let output = lcr
            .entries
            .into_iter()
            .map(|it| it.name)
            // Canonicalise output dirsep
            .map(|n| n.replace(MAIN_SEPARATOR, "/"))
            .collect::<Vec<_>>();

        eprintln!("Canonicalised output: {output:?}");
        // We are using walkdir in depth-first mode. That is to say, directories appear before the files within.
        // However, the order of files within any directory may vary (this seems to be a libc thing).

        // Therefore, we have two checks:
        // - The output data set is same as the expected, **but in any order**;
        // - Every item in the output is preceded by its parent.

        // Contents check: sort both, test for equality.
        {
            let mut e_sorted = expected.to_vec();
            e_sorted.sort_unstable();
            let mut o_sorted = output.clone();
            o_sorted.sort();
            assert_eq!(e_sorted, o_sorted);
        }

        // Parent check: Use a hashset to confirm we've already seen each item's parent.
        let mut seen = HashSet::new();
        for item in output {
            // Strip the output directory prefix as not relevant to the check
            let it = item
                .strip_prefix(dir_prefix)
                .expect("output item did not contain expected prefix");
            // Strip the leading slash
            let it = it.strip_prefix('/').unwrap_or(it);
            // Compute the parent, if present
            let split = it.split_once('/');
            if let Some((parent, _leaf)) = split {
                assert!(
                    seen.contains(parent),
                    "Item {item} seen before its parent {parent}"
                );
            } // else it is at the root, so no check required

            let _ = seen.insert(it.to_string());
        }
    }

    #[tokio::test]
    async fn no_recurse() {
        let result = LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("d");
            let _ = tray.make_dir("d/d2");
            let _ = tray.make_dir("d/d2/e");
            let _ = tray.make_dir("d/d2/e/f");
            let _ = tray.create_text("d/d2/hi", "hi");
            let _ = tray.make_dir("d/d2/x");
            let _ = tray.create_text("d/d2/x/xyzy", "hi");
            let _ = tray.create_text("f", "no");
            let _ = tray.make_dir("no");

            test_ls_main("d/d2", false).await
        })
        .await
        .unwrap();
        expected_result(result, "d/d2", &["d/d2", "d/d2/hi", "d/d2/e", "d/d2/x"]);
    }

    #[tokio::test]
    async fn recurse() {
        let result = LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("d");
            let _ = tray.make_dir("d/d2");
            let _ = tray.make_dir("d/d2/e");
            let _ = tray.make_dir("d/d2/e/f");
            let _ = tray.create_text("d/d2/hi", "hi");
            let _ = tray.make_dir("d/d2/x");
            let _ = tray.create_text("d/d2/x/xyzy", "hi");
            let _ = tray.create_text("f", "no");
            let _ = tray.make_dir("no");

            test_ls_main("d/d2", true).await
        })
        .await
        .unwrap();
        eprintln!("Result: {result:?}");
        expected_result(
            result,
            "d/d2",
            &[
                "d/d2",
                "d/d2/e",
                "d/d2/e/f",
                "d/d2/hi",
                "d/d2/x",
                "d/d2/x/xyzy",
            ],
        );
    }

    #[tokio::test]
    async fn not_found() {
        let result = LitterTray::try_with_async(async |tray| {
            let _ = tray.make_dir("d");
            test_ls_main("xyzy", true).await
        })
        .await
        .unwrap();
        assert_eq!(
            Status::try_from(result.status).unwrap(),
            Status::FileNotFound
        );
    }
}
