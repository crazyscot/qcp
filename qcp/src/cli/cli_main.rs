//! Main CLI for qcp
// (c) 2024 Ross Younger

use std::io::Write as _;
use std::process::ExitCode;

use super::args::CliArgs;
use crate::{
    cli::styles::{RESET, configure_colours, error, use_colours},
    client::MAX_UPDATE_FPS,
    config::{Configuration, Manager},
    os::{self, AbstractPlatform as _},
};

use indicatif::{MultiProgress, ProgressDrawTarget};
use lessify::OutputPaged;

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
                format!("{ERROR}Error:{RESET} {e}", ERROR = error()).output_paged();
            }
            ExitCode::FAILURE
        }
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
    }
}

/// Inner CLI entrypoint
///
/// # Safety
/// - This function starts a tokio runtime.
/// - This function is not safe to call from multi-threaded code.
fn cli_inner() -> anyhow::Result<bool> {
    let args = match CliArgs::custom_parse(std::env::args_os()) {
        Ok(args) => args,
        Err(e) => {
            match e.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => {
                    // this is a normal exit
                    let message = e.render();
                    if use_colours() {
                        message.ansi().output_paged();
                    } else {
                        message.output_paged();
                    }
                    return Ok(true);
                }
                _ => (),
            }
            // this is an error
            anyhow::bail!(e);
        }
    };

    let mode = MainMode::from(&args); // side-effect: holds progress bar, if we need one

    // Now fold the arguments in with the CLI config (which may fail)
    // (to provoke an error here: `qcp host: host2:`)
    let mut config_manager = Manager::try_from(&args)?;
    let colours = config_manager.get_color(Some(Configuration::system_default().color))?;
    configure_colours(Some(colours));

    match mode {
        MainMode::HelpBuffers => {
            // Decided not to send this to pager as it is rich with emoji,
            // which doesn't page well on Windows.

            // Write instead of print to avoid the possibility of a panic
            // when externally piped to another program.
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                os::Platform::help_buffers_mode(
                    Configuration::recv_buffer(),
                    Configuration::send_buffer(),
                )
            );
            Ok(true)
        }
        MainMode::ShowConfigFiles => {
            println!("{:?}", Manager::config_files());
            Ok(true)
        }
        MainMode::ShowConfig => {
            config_manager.apply_system_default();
            format!(
                "Client configuration:\n{}",
                config_manager.to_display_adapter::<Configuration>()
            )
            .output_paged();
            config_manager.validate_configuration()?;
            Ok(true)
        }
        MainMode::Server => Ok(run_in_tokio(crate::server_main).map_or_else(
            |e| {
                eprintln!("{ERROR}ERROR{RESET} Server: {e:?}", ERROR = error());
                false
            },
            |()| true,
        )),
        MainMode::Client(progress) => {
            config_manager.validate_configuration()?;
            // this mode may return false
            run_in_tokio(|| crate::client_main(&mut config_manager, progress, args.client_params))
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn run_in_tokio<R, F: AsyncFnOnce() -> R>(func: F) -> R {
    func().await
}
