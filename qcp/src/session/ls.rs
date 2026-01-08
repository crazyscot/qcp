//! List Contents command (remote directory listing)
// (c) 2025 Ross Younger

use anyhow::Result;
use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tracing::{debug, error, trace};
use walkdir::WalkDir;

use crate::Parameters;
use crate::protocol::common::{ProtocolMessage, ReceivingStream, SendingStream};
use crate::protocol::session::{ListArgs, ListData, ListEntry};
use crate::protocol::session::{ResponseV1, prelude::*};
use crate::session::common::{FindOption as _, send_ok};
use crate::session::handler::{CommandHandler, SessionCommandInner};
use crate::session::{CommandStats, RequestResult, error_and_return};

pub(crate) struct ListingHandler;

#[async_trait]
impl CommandHandler for ListingHandler {
    type Args = ListArgs;

    async fn send_impl<'a, S: SendingStream, R: ReceivingStream>(
        &mut self,
        inner: &mut SessionCommandInner<'a, S, R>,
        job: &crate::client::CopyJobSpec,
        params: Parameters,
    ) -> Result<RequestResult> {
        anyhow::ensure!(
            inner.compat.supports(Feature::MKDIR_SETMETA_LS),
            "Operation not supported by remote"
        );
        let path = &job.source.filename; // yes, source filename

        // This is a trivial operation, we do not bother with a progress bar.
        trace!("sending command");
        let mut outbound = &mut inner.stream.send;
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
        let result = Response::from_reader_async_framed(&mut inner.stream.recv).await?;
        if result.status() != Status::Ok {
            error!("List failed: {:?}", result);
            return Err(anyhow::Error::new(result));
        }
        let mut data = vec![];
        loop {
            let packet = ListData::from_reader_async_framed(&mut inner.stream.recv)
                .await
                .map_err(|r| anyhow::anyhow!("failed to parse List response: {r}"))?;
            let another = packet.more_to_come;
            data.push(packet);
            if !another {
                break;
            }
        }
        let data = ListData::join(data);
        Ok(RequestResult::new(CommandStats::default(), Some(data)))
    }

    async fn handle_impl<'a, S: SendingStream, R: ReceivingStream>(
        &mut self,
        inner: &mut SessionCommandInner<'a, S, R>,
        args: &ListArgs,
    ) -> Result<()> {
        let path = &args.path;
        let recurse = args.options.find_option(CommandParam::Recurse).is_some();
        let stream = &mut inner.stream;
        // debug!("ls: path {path}, recurse={recurse}");

        let res = tokio::fs::metadata(path).await;
        let meta = match res {
            Ok(meta) => meta,
            Err(e) => {
                error_and_return!(stream, e);
            }
        };
        if meta.is_file() {
            let data = ListData {
                entries: vec![ListEntry {
                    name: path.clone(),
                    directory: false,
                    size: Uint(meta.len()),
                    attributes: vec![],
                }],
                more_to_come: false,
            };

            Response::V1(ResponseV1 {
                status: Status::Ok.into(),
                message: None,
            })
            .to_writer_async_framed(&mut stream.send)
            .await?;
            return data.to_writer_async_framed(&mut stream.send).await;
        }
        let entries: Result<Vec<_>, walkdir::Error> = WalkDir::new(path)
            // do NOT omit the root here, recursive transfer depends on it to mkdir the top-level dir
            .max_depth(if recurse { usize::MAX } else { 1 })
            .follow_links(true)
            .into_iter()
            .map(|e| e.map(ListEntry::from))
            .collect();

        let list = match entries {
            Ok(v) => ListData {
                entries: v,
                more_to_come: false,
            },
            Err(e) => {
                debug!("ls: walkdir error: {e}");
                error_and_return!(stream, e);
            }
        };
        // debug!("ls: sending response {}", list);

        // Careful! The response might be too long for a Response packet (64k).
        let packets = list.split_by_size(ListData::WIRE_ENCODING_LIMIT)?;
        send_ok(&mut stream.send).await?;
        for p in packets {
            p.to_writer_async_framed(&mut stream.send).await?;
        }
        stream.send.flush().await?;
        trace!("complete");
        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use std::collections::HashSet;
    use std::path::MAIN_SEPARATOR;

    use crate::protocol::session::{ListData, prelude::*};
    use crate::session::SessionCommandImpl as _;
    use crate::{
        Configuration, Parameters,
        client::CopyJobSpec,
        protocol::test_helpers::{new_test_plumbing, read_from_stream},
        session::handler::{ListingHandler, SessionCommand},
    };
    use anyhow::{Result, bail, ensure};
    use littertray::LitterTray;
    use pretty_assertions::assert_eq;

    async fn test_ls_main(path: &str, recurse: bool, expect_success: bool) -> Result<ListData> {
        let (pipe1, mut pipe2) = new_test_plumbing();
        let mut sender = SessionCommand::boxed(
            pipe1,
            ListingHandler,
            None,
            Compatibility::Level(4),
            Configuration::system_default(),
            None,
        );
        let spec =
            CopyJobSpec::from_parts(path, &format!("desthost:{path}"), false, false).unwrap();
        let params = Parameters {
            recurse,
            ..Default::default()
        };
        let result = {
            // this subscope forces sender_fut to unborrow sender.
            let sender_fut = sender.send(&spec, params);
            tokio::pin!(sender_fut);

            let result = read_from_stream(&mut pipe2.recv, &mut sender_fut).await;
            let cmd = result.expect_left("sender should not have completed early")?;
            let Command::List(args) = cmd else {
                bail!("expected CreateDirectory command");
            };

            let mut handler = SessionCommand::boxed(
                pipe2,
                ListingHandler,
                Some(args),
                Compatibility::Level(4),
                Configuration::system_default(),
                None,
            );
            let (r1, r2) = tokio::join!(sender_fut, handler.handle());
            r2.expect("handler should not have failed");
            match r1 {
                Ok(it) => {
                    ensure!(expect_success, "sender should have failed");
                    it
                }
                Err(e) => {
                    ensure!(!expect_success, "sender should not have failed");
                    return Err(e);
                }
            }
        };
        Ok(result.list.expect("expected ListData in result"))
    }

    // Check for expected results, allowing for walkdir and libc variations.
    fn expected_result(lcr: ListData, dir_prefix: &str, expected: &[&str]) {
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

            test_ls_main("d/d2", false, true).await
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

            test_ls_main("d/d2", true, true).await
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
            test_ls_main("xyzy", true, false).await
        })
        .await
        .unwrap_err();
        assert!(result.to_string().contains("FileNotFound"));
    }
}
