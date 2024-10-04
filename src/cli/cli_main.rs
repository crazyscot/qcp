// Main CLI entrypoint
// (c) 2024 Ross Younger

use std::process::ExitCode;

use super::args::CliArgs;

use crate::{client, os::os, server, transport, util::setup_tracing};
use clap::Parser;
use indicatif::MultiProgress;
use tracing::error_span;

/// Main CLI entrypoint
pub fn cli() -> anyhow::Result<ExitCode> {
    let args = CliArgs::parse();
    if args.help_buffers {
        // One day we might make this a function of the remote host.
        let send_window = transport::SEND_BUFFER_SIZE;
        #[allow(clippy::cast_possible_truncation)]
        let recv_window =
            transport::practical_receive_window_for(args.bandwidth_bytes()?, args.rtt)? as usize;
        os::print_udp_buffer_size_help_message(recv_window, send_window);
        return Ok(ExitCode::SUCCESS);
    }
    if args.server {
        return server_main(&args);
    }
    client_main(&args)
}

fn client_main(args: &CliArgs) -> anyhow::Result<ExitCode> {
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

    client::client_main(args, &progress)
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

fn server_main(args: &CliArgs) -> anyhow::Result<ExitCode> {
    let trace_level = if args.debug { "trace" } else { "error" };
    setup_tracing(trace_level, None, &args.log_file).inspect_err(|e| eprintln!("{e:?}"))?;
    let _span = error_span!("SERVER").entered();

    server::main(args).map(|()| ExitCode::SUCCESS).map_err(|e| {
        tracing::error!("{e}");
        // TODO: Decide about error handling. For now detailed anyhow output is fine.
        e
    })
}
