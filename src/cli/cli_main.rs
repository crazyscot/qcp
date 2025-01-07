//! Main CLI for qcp
// (c) 2024 Ross Younger

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
    HelpBuffers,
    ShowConfigFiles,
}

impl From<&CliArgs> for MainMode {
    fn from(args: &CliArgs) -> Self {
        if args.server {
            MainMode::Server
        } else if args.show_config {
            MainMode::ShowConfig
        } else if args.help_buffers {
            MainMode::HelpBuffers
        } else if args.config_files {
            MainMode::ShowConfigFiles
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
/// # Return
/// true indicates success. false indicates a failure we have logged. An Error is a failure we have not output or logged.
#[tokio::main(flavor = "current_thread")]
#[allow(clippy::missing_panics_doc)]
pub async fn cli() -> anyhow::Result<bool> {
    let args = CliArgs::custom_parse();
    let mode = MainMode::from(&args); // side-effect: holds progress bar, if we need one

    // Now fold the arguments in with the CLI config (which may fail)
    // (to provoke an error here: `qcp host: host2:`)
    let config_manager = Manager::try_from(&args)?;
    let config = config_manager.get::<Configuration>()?.validate()?;

    setup_tracing(
        trace_level(&args.client_params),
        mode.progress(),
        &args.client_params.log_file,
        config.time_format,
    )?; // to provoke error: set RUST_LOG=.

    match mode {
        MainMode::HelpBuffers => {
            os::print_udp_buffer_size_help_message(
                Configuration::recv_buffer(),
                Configuration::send_buffer(),
            );
        }
        MainMode::ShowConfigFiles => {
            println!("{:?}", Manager::config_files());
        }
        MainMode::ShowConfig => {
            println!("{}", config_manager.to_display_adapter::<Configuration>());
        }
        MainMode::Server => {
            let _span = error_span!("REMOTE").entered();
            server_main(&config).await?;
        }
        MainMode::Client(progress) => {
            // this mode may return false
            return client_main(&config, progress, args.client_params).await;
        }
    };
    Ok(true)
}
