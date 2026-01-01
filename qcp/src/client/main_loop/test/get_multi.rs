//! Tests for recursive get operations.
// (c) 2025 Ross Younger
// NOTE: This file is within a module gated by #[cfg(test)] and #[cfg_attr(coverage_nightly, coverage(off))]
//!
//! This module covers a combinatoric explosion of multi-file GET scenarios:
//! - single vs multi-source
//! - destination exists vs does not exist
//! - source is absolute path vs relative path
//! - destination is absolute path vs relative path
//!
//! It also covers the situation where you have asked to recurse on a source that is really a file.

use crate::{
    Configuration,
    client::main_loop::Client,
    config::Configuration_Optional,
    protocol::{control::Compatibility, test_helpers::new_test_plumbing},
};
use indicatif::{MultiProgress, ProgressBar, ProgressDrawTarget};
use littertray::LitterTray;
use rstest::*;
use std::{cell::RefCell, path::MAIN_SEPARATOR, time::Duration};
use walkdir::WalkDir;

fn setup_fs(tray: &mut LitterTray) {
    let _ = tray.make_dir("s/src1");
    let _ = tray.make_dir("s/src2");
    let _ = tray.make_dir("s/src3");
    let _ = tray.make_dir("s/src1/subdir");
    let _ = tray.create_text("s/src1/file1.txt", "file1 contents");
    let _ = tray.create_text("s/src1/subdir/file2.txt", "file2 contents");
    let _ = tray.create_text("s/src2/file3.txt", "file3 contents");
    let _ = tray.create_text("s/src2/file4.txt", "file3 contents");
    let _ = tray.create_text("s/src3/file5.txt", "file4 contents");
}

const ALL_SOURCES: &[&str] = &["s/src1", "s/src2", "s/src3"];
/// Additive list of expected outdir contents for each n_sources
const EXPECTED_CONTENTS: &[&[&str]] = &[
    &[""], // corresponds to the output directory
    &[
        "src1",
        "src1/subdir",
        "src1/file1.txt",
        "src1/subdir/file2.txt",
    ],
    &["src2", "src2/file3.txt", "src2/file4.txt"],
];

const OUTPUT_DIRECTORY: &str = "d/outdir"; // relative to littertray

struct LocalTracing {}
#[fixture]
fn shared_setup_tracing() -> LocalTracing {
    use crate::util::{ConsoleTraceType, TimeFormat, setup_tracing};
    setup_tracing(
        "debug",
        ConsoleTraceType::Standard,
        None,
        TimeFormat::default(),
        true,
    )
    .unwrap();
    LocalTracing {}
}

async fn run_plumbing(uut: &mut Client) -> anyhow::Result<(bool, crate::session::CommandStats)> {
    let cfg = Configuration_Optional::default();
    let n_streams = 10;

    // We will set up a number of paired pairs of test streams.
    // One half of each pair will be server-side handlers, the other half will be usable by the client side.
    let client_streams = RefCell::new(Vec::with_capacity(n_streams));
    let handlers = RefCell::new(Vec::with_capacity(n_streams));
    {
        let mut v_client = client_streams.borrow_mut();
        let mut v_handlers = handlers.borrow_mut();
        for idx in 0..n_streams {
            let (p1, p2) = new_test_plumbing();
            v_client.push(p1);
            let jh = tokio::spawn(async move {
                if let Err(e) = crate::server::handle_stream(
                    p2,
                    Compatibility::Level(4),
                    crate::util::io::DEFAULT_COPY_BUFFER_SIZE,
                )
                .await
                {
                    eprintln!("stream handler {idx} failed: {e}");
                } else {
                    eprintln!("stream handler {idx} done");
                }
            });
            v_handlers.push(jh);
        }
        v_client.reverse(); // so the first one popped is #0, etc., for better understandability.
    }

    let prep_result = uut.prep(&cfg, Configuration::system_default()).unwrap();

    uut.process_recursive_get(
        &prep_result.job_specs,
        async || {
            Ok(client_streams
                .borrow_mut()
                .pop()
                .expect("Ran out of streams!"))
        },
        |stream_pair, job, filename_width, pass| {
            uut.run_request(stream_pair, job, filename_width, pass)
        },
    )
    .await
}

#[rstest]
#[timeout(Duration::from_secs(1))]
#[tokio::test]
async fn get_multi(
    #[allow(unused_variables)] shared_setup_tracing: LocalTracing,
    #[values(1, 2)] n_sources: usize,
    #[values(true, false)] dest_exists: bool,
    #[values(true, false)] src_absolute: bool,
    #[values(true, false)] dest_absolute: bool,
) {
    let should_succeed = n_sources == 1 || dest_exists;

    LitterTray::try_with_async(async move |tray| {
        setup_fs(tray);
        if dest_exists {
            let _ = tray.make_dir(OUTPUT_DIRECTORY)?;
        }
        let tray = tray.directory().to_str().unwrap();

        let sources = ALL_SOURCES[..n_sources]
            .iter()
            .map(|s| {
                if src_absolute {
                    format!("{tray}/{s}")
                } else {
                    (*s).to_string()
                }
            })
            .map(|s| format!("127.0.0.1:{s}"))
            .collect::<Vec<_>>();
        let sources_strs = sources
            .iter()
            .map(std::string::String::as_str)
            .collect::<Vec<&str>>();

        let dest = if dest_absolute {
            format!("{tray}/{OUTPUT_DIRECTORY}")
        } else {
            OUTPUT_DIRECTORY.to_string()
        };

        // We need the tray path to be able to work with absolute paths.
        let mut uut = super::make_uut_multi(|_, _| (), &sources_strs, &dest, 4);
        uut.args.client_params.remote_debug = true;
        uut.display = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());
        uut.spinner = ProgressBar::hidden();
        uut.args.client_params.recurse = true;

        let result = run_plumbing(&mut uut).await;

        if should_succeed {
            use std::collections::HashSet;

            let result = result.expect("This operation should have succeeded");
            eprintln!("result: {result:?}");
            // Compute expected contents
            let mut raw = vec![];
            for this_level in EXPECTED_CONTENTS.iter().take(n_sources + 1) {
                let mut tmp = this_level.iter().collect::<Vec<_>>();
                raw.append(&mut tmp);
            }
            let mut expected = HashSet::new();
            for ent in &raw {
                let mut s = String::from(OUTPUT_DIRECTORY);
                if !ent.is_empty() {
                    if n_sources == 1 && !dest_exists {
                        // if 1 source and the destination does not exist, scp does not create the first component of the output dir - and nor do we.
                        if let Some((_, leaf)) = ent.split_once('/') {
                            s.push('/');
                            s.push_str(leaf);
                        } else {
                            // no slash? it's the top dir of the source; omit it.
                            //s.push_str(ent);
                        }
                    } else {
                        s.push('/');
                        s.push_str(ent);
                    }
                }
                let _ = expected.insert(s); // unchecked, as duplicates do legitimately arise when n_sources==1 && !dest_exists
            }

            // Now walk the output directory ...
            let walk = WalkDir::new(OUTPUT_DIRECTORY);
            let mut actual = HashSet::new();
            for ent in walk {
                let ent = ent.unwrap();
                let path = ent.path().to_str().unwrap().to_string();
                // Canonicalise slashes, as we've written for Linux but get backslashes on Windows
                let path = path.replace(MAIN_SEPARATOR, "/");
                assert!(actual.insert(path));
            }
            assert_eq!(expected, actual, "expected set != actual");
        } else {
            let _ = result.expect_err("This operation should have failed");
        }
        Ok(())
    })
    .await
    .unwrap();
}

#[rstest]
#[timeout(Duration::from_secs(1))]
#[tokio::test]
async fn get_multi_single_file(
    #[allow(unused_variables)] shared_setup_tracing: LocalTracing,
    #[values(true, false)] dest_exists: bool,
) {
    LitterTray::try_with_async(async move |tray| {
        use std::path::MAIN_SEPARATOR_STR;

        setup_fs(tray);
        if dest_exists {
            let _ = tray.make_dir(OUTPUT_DIRECTORY)?;
        } else {
            let _ = tray.make_dir("d");
        }

        let sources = vec!["127.0.0.1:s/src1/file1.txt"];
        let src_file = "s/src1/file1.txt".replace('/', MAIN_SEPARATOR_STR);
        let dest = OUTPUT_DIRECTORY.to_string();

        let mut uut = super::make_uut_multi(|_, _| (), &sources, &dest, 4);
        uut.args.client_params.remote_debug = true;
        uut.display = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());
        uut.spinner = ProgressBar::hidden();
        uut.args.client_params.recurse = true;

        let result = run_plumbing(&mut uut).await;

        let result = result.expect("This operation should have succeeded");
        eprintln!("result: {result:?}");

        let src_meta = std::fs::metadata(src_file).unwrap();

        let expected_file_path = if dest_exists {
            format!("{OUTPUT_DIRECTORY}{MAIN_SEPARATOR}file1.txt")
        } else {
            dest
        };
        let meta = std::fs::metadata(&expected_file_path).unwrap();
        assert_eq!(meta.len(), src_meta.len());
        Ok(())
    })
    .await
    .unwrap();
}
