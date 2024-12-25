//! Main CLI entrypoint for qcp
// (c) 2024 Ross Younger

use std::process::ExitCode;

use super::{args::CliArgs, styles::ERROR_S};
use crate::{
    client::{client_main, Parameters as ClientParameters, MAX_UPDATE_FPS},
    config::{Configuration, Manager},
    os,
    server::server_main,
    util::setup_tracing,
};

use anstream::{eprintln, println};
use indicatif::{MultiProgress, ProgressDrawTarget};
use owo_colors::OwoColorize as _;
use tracing::error_span;

/// Computes the trace level for a given set of [ClientParameters]
fn trace_level(args: &ClientParameters) -> &str {
    if args.debug {
        "debug"
    } else if args.quiet {
        "error"
    } else {
        "info"
    }
}

/// Main CLI entrypoint
///
/// Call this from `main`. It reads argv.
/// # Exit status
/// 0 indicates success; non-zero indicates failure.
#[tokio::main(flavor = "current_thread")]
#[allow(clippy::missing_panics_doc)]
pub async fn cli() -> anyhow::Result<ExitCode> {
    let args = CliArgs::custom_parse();
    if args.help_buffers {
        os::print_udp_buffer_size_help_message(
            Configuration::recv_buffer(),
            Configuration::send_buffer(),
        );
        return Ok(ExitCode::SUCCESS);
    }

    let progress = (!args.server).then(|| {
        MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(MAX_UPDATE_FPS))
    });

    if args.config_files {
        // do this before attempting to read config, in case it fails
        println!("{:?}", Manager::config_files());
        return Ok(ExitCode::SUCCESS);
    }

    // Now fold the arguments in with the CLI config (which may fail)
    let config_manager = match Manager::try_from(&args) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("{}: {err}", "ERROR".style(*ERROR_S));
            return Ok(ExitCode::FAILURE);
        }
    };

    let config = match config_manager.get::<Configuration>() {
        Ok(c) => c,
        Err(err) => {
            eprintln!("{}: Failed to parse configuration", "ERROR".style(*ERROR_S));
            err.into_iter().for_each(|e| eprintln!("{e}"));
            return Ok(ExitCode::FAILURE);
        }
    };

    setup_tracing(
        trace_level(&args.client_params),
        progress.as_ref(),
        &args.client_params.log_file,
        config.time_format,
    )
    .inspect_err(|e| eprintln!("{e:?}"))?;

    if args.show_config {
        println!("{}", config_manager.to_display_adapter::<Configuration>());
        Ok(ExitCode::SUCCESS)
    } else if args.server {
        let _span = error_span!("REMOTE").entered();
        server_main(&config)
            .await
            .map(|()| ExitCode::SUCCESS)
            .inspect_err(|e| tracing::error!("{e}"))
    } else {
        client_main(&config, progress.unwrap(), args.client_params)
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
