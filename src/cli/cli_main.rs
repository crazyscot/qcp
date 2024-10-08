// Main CLI entrypoint
// (c) 2024 Ross Younger

use std::process::ExitCode;

use super::args::CliArgs;

use crate::{
    client::client_main,
    os::os,
    server::server_main,
    transport::{BandwidthConfig, BandwidthParams},
    util::setup_tracing,
};
use clap::Parser;
use indicatif::MultiProgress;
use tracing::error_span;

/// Main CLI entrypoint
pub fn cli() -> anyhow::Result<ExitCode> {
    let args = CliArgs::parse();
    if args.help_buffers {
        // One day we might make this a function of the remote host.
        let buffer_config = BandwidthConfig::from(BandwidthParams::from(&args));
        os::print_udp_buffer_size_help_message(
            buffer_config.recv_buffer,
            buffer_config.send_buffer,
        );
        return Ok(ExitCode::SUCCESS);
    }
    if args.server {
        return run_server(&args);
    }
    run_client(&args)
}

#[tokio::main(flavor = "current_thread")]
async fn run_client(args: &CliArgs) -> anyhow::Result<ExitCode> {
    let progress = MultiProgress::new(); // This writes to stderr
    let trace_level = if args.debug {
        "trace"
    } else if args.quiet {
        "error"
    } else {
        "info"
    };
    setup_tracing(trace_level, Some(&progress), &args.log_file)
        .inspect_err(|e| eprintln!("{e:?}"))?;

    client_main(args, &progress)
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

#[tokio::main(flavor = "current_thread")]
async fn run_server(args: &CliArgs) -> anyhow::Result<ExitCode> {
    let trace_level = if args.debug { "trace" } else { "error" };
    setup_tracing(trace_level, None, &args.log_file).inspect_err(|e| eprintln!("{e:?}"))?;
    let _span = error_span!("SERVER").entered();

    server_main(args)
        .await
        .map(|()| ExitCode::SUCCESS)
        .map_err(|e| {
            tracing::error!("{e}");
            // TODO: Decide about error handling. For now detailed anyhow output is fine.
            e
        })
}
