//! Main CLI entrypoint for qcp
// (c) 2024 Ross Younger

use std::process::ExitCode;

use super::args::CliArgs;

use crate::{
    client::{client_main, MAX_UPDATE_FPS},
    os,
    server::server_main,
    util::setup_tracing,
};
use clap::Parser;
use indicatif::{MultiProgress, ProgressDrawTarget};
use tracing::error_span;

/// Main CLI entrypoint
///
/// Call this from `main`. It reads argv.
/// # Exit status
/// 0 indicates success; non-zero indicates failure.
#[tokio::main(flavor = "current_thread")]
#[allow(clippy::missing_panics_doc)]
pub async fn cli() -> anyhow::Result<ExitCode> {
    let args = CliArgs::parse();
    if args.help_buffers {
        os::print_udp_buffer_size_help_message(
            args.bandwidth.recv_buffer(),
            args.bandwidth.send_buffer(),
        );
        return Ok(ExitCode::SUCCESS);
    }
    let trace_level = if args.debug {
        "debug"
    } else if args.client.quiet {
        "error"
    } else {
        "info"
    };
    let progress = if args.server {
        None
    } else {
        Some(MultiProgress::with_draw_target(
            ProgressDrawTarget::stderr_with_hz(MAX_UPDATE_FPS),
        ))
    };
    setup_tracing(trace_level, progress.as_ref(), &args.log_file)
        .inspect_err(|e| eprintln!("{e:?}"))?;

    if args.server {
        let _span = error_span!("REMOTE").entered();
        server_main(args.bandwidth, args.quic)
            .await
            .map(|()| ExitCode::SUCCESS)
            .inspect_err(|e| tracing::error!("{e}"))
    } else {
        client_main(args.client, args.bandwidth, args.quic, progress.unwrap())
            .await
            .inspect_err(|e| tracing::error!("{e}"))
            .or_else(|_| Ok(false))
            .map(|success| {
                if success {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            })
    }
}
