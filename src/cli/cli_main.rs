//! Main CLI entrypoint for qcp
// (c) 2024 Ross Younger

use std::process::ExitCode;

use super::args::CliArgs;
use crate::{
    client::{client_main, Parameters as ClientParameters, MAX_UPDATE_FPS},
    config::{Configuration, Manager},
    os,
    server::server_main,
    util::setup_tracing,
};

use anstream::println;
use indicatif::{MultiProgress, ProgressDrawTarget};
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

enum MainMode {
    Server,
    Client(MultiProgress),
    ShowConfig,
}

impl From<&CliArgs> for MainMode {
    fn from(args: &CliArgs) -> Self {
        if args.server {
            MainMode::Server
        } else if args.show_config {
            MainMode::ShowConfig
        } else {
            MainMode::Client(MultiProgress::with_draw_target(
                ProgressDrawTarget::stderr_with_hz(MAX_UPDATE_FPS),
            ))
        }
    }
}

impl MainMode {
    fn progress(&self) -> Option<&MultiProgress> {
        match self {
            MainMode::Client(mp) => Some(mp),
            _ => None,
        }
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
    if args.config_files {
        // do this before attempting to read config, in case it fails
        setup_tracing(
            trace_level(&args.client_params),
            None,
            &None,
            args.config.time_format.unwrap_or_default(),
        )?;
        println!("{:?}", Manager::config_files());
        return Ok(ExitCode::SUCCESS);
    }

    let main_mode = MainMode::from(&args); // side-effect: holds progress bar, if we need one

    // Now fold the arguments in with the CLI config (which may fail)
    // (to provoke an error here: `qcp host: host2:`)
    let config_manager = Manager::try_from(&args)?;
    let config = config_manager.get::<Configuration>()?.validate()?;

    setup_tracing(
        trace_level(&args.client_params),
        main_mode.progress(),
        &args.client_params.log_file,
        config.time_format,
    )?; // to provoke error: set RUST_LOG=.

    match main_mode {
        MainMode::ShowConfig => {
            println!("{}", config_manager.to_display_adapter::<Configuration>());
            return Ok(ExitCode::SUCCESS);
        }
        MainMode::Server => {
            let _span = error_span!("REMOTE").entered();
            server_main(&config).await?;
            return Ok(ExitCode::SUCCESS);
        }
        MainMode::Client(progress) => {
            return client_main(&config, progress, args.client_params)
                .await
                .map(|success| {
                    if success {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                });
        }
    };
}
