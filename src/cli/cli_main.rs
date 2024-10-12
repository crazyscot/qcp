// Main CLI entrypoint
// (c) 2024 Ross Younger

use std::process::ExitCode;

use super::args::CliArgs;

use crate::{
    client::client_main, os, server::server_main, transport::BandwidthConfig, util::setup_tracing,
};
use clap::Parser;
use indicatif::{MultiProgress, ProgressDrawTarget};
use tracing::error_span;

/// Main CLI entrypoint
#[tokio::main(flavor = "current_thread")]
#[allow(clippy::missing_panics_doc)]
pub async fn cli() -> anyhow::Result<ExitCode> {
    let args = CliArgs::parse();
    if args.help_buffers {
        let buffer_config = BandwidthConfig::from(&args.bandwidth);
        os::print_udp_buffer_size_help_message(
            buffer_config.recv_buffer,
            buffer_config.send_buffer,
        );
        return Ok(ExitCode::SUCCESS);
    }
    let trace_level = if args.debug {
        "debug"
    } else if args.quiet {
        "error"
    } else {
        "info"
    };
    let progress = if args.server {
        None
    } else {
        Some(MultiProgress::with_draw_target(
            ProgressDrawTarget::stderr_with_hz(crate::console::MAX_UPDATE_FPS),
        ))
    };
    setup_tracing(trace_level, progress.as_ref(), &args.log_file)
        .inspect_err(|e| eprintln!("{e:?}"))?;

    if args.server {
        let _span = error_span!("REMOTE").entered();
        server_main(&args)
            .await
            .map(|()| ExitCode::SUCCESS)
            .inspect_err(|e| tracing::error!("{e}"))
    } else {
        client_main(&args, progress.unwrap())
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
