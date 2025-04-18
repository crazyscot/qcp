//! Main CLI for qcp
// (c) 2024 Ross Younger

use std::process::ExitCode;

use super::args::CliArgs;
use crate::{
    cli::styles::{ERROR, RESET},
    client::MAX_UPDATE_FPS,
    config::{Configuration, Manager},
    os::{self, AbstractPlatform as _},
};

use anstream::println;
use indicatif::{MultiProgress, ProgressDrawTarget};

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

/// Main CLI entrypoint
///
/// Call this from `main`. It reads argv.
/// # Return
/// true indicates success. false indicates a failure (we have output to stderr).
#[must_use]
pub fn cli() -> ExitCode {
    match cli_inner() {
        Err(e) => {
            if crate::util::tracing_is_initialised() {
                tracing::error!("{e}");
            } else {
                eprintln!("{ERROR}Error:{RESET} {e}");
            }
            ExitCode::FAILURE
        }
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
    }
}

/// Inner CLI entrypoint
#[tokio::main(flavor = "current_thread")]
async fn cli_inner() -> anyhow::Result<bool> {
    let args = CliArgs::custom_parse();
    let mode = MainMode::from(&args); // side-effect: holds progress bar, if we need one

    // Now fold the arguments in with the CLI config (which may fail)
    // (to provoke an error here: `qcp host: host2:`)
    let mut config_manager = Manager::try_from(&args)?;

    match mode {
        MainMode::HelpBuffers => {
            os::Platform::help_buffers_mode(
                Configuration::recv_buffer(),
                Configuration::send_buffer(),
            );
            Ok(true)
        }
        MainMode::ShowConfigFiles => {
            println!("{:?}", Manager::config_files());
            Ok(true)
        }
        MainMode::ShowConfig => {
            config_manager.apply_system_default();
            println!(
                "Client configuration:\n{}",
                config_manager.to_display_adapter::<Configuration>()
            );
            Ok(true)
        }
        MainMode::Server => Ok(crate::server_main().await.map_or_else(
            |e| {
                eprintln!("{ERROR}ERROR{RESET} Server: {e:?}");
                false
            },
            |()| true,
        )),
        MainMode::Client(progress) => {
            // this mode may return false
            crate::client_main(
                &mut config_manager.validate_configuration()?,
                progress,
                args.client_params,
            )
            .await
        }
    }
}
