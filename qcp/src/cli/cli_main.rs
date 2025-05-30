//! Main CLI for qcp
// (c) 2024 Ross Younger

use std::io::Write as _;
use std::process::ExitCode;

use super::args::CliArgs;
use crate::{
    Parameters,
    cli::styles::{RESET, configure_colours, error, use_colours},
    client::MAX_UPDATE_FPS,
    config::{Configuration, Manager},
    os::{self, AbstractPlatform as _},
};

use anyhow::Result;
use indicatif::{MultiProgress, ProgressDrawTarget};
use lessify::OutputPaged;

#[derive(PartialEq, Clone, Copy, Debug)]
enum MainMode {
    Server,
    Client,
    ShowConfig,
    HelpBuffers,
    ShowConfigFiles,
    ListFeatures,
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
        } else if args.list_features {
            MainMode::ListFeatures
        } else {
            MainMode::Client
        }
    }
}

/// Main CLI entrypoint
///
/// Call this from `main`. It reads argv.
///
/// # Safety
/// - This function may start a tokio runtime and perform work in it.
/// - This function is not safe to call from multi-threaded code.
#[must_use]
pub fn cli() -> ExitCode {
    #[allow(clippy::match_bool)] // improved readability
    cli_inner()
        .inspect_err(|e| {
            if crate::util::tracing_is_initialised() {
                tracing::error!("{e}");
            } else {
                format!("{ERROR}Error:{RESET} {e}", ERROR = error()).output_paged();
            }
        })
        .map_or(ExitCode::FAILURE, |success| match success {
            true => ExitCode::SUCCESS,
            false => ExitCode::FAILURE,
        })
}

/// Inner CLI logic
///
/// # Return
/// true indicates success. false indicates a failure where the callee has output to stderr.
///
/// # Note
/// - This function starts a tokio runtime and performs work in it.
fn cli_inner() -> Result<bool> {
    let Some(args) = parse_args()? else {
        return Ok(true); // help/version shown; exit
    };

    let mode = MainMode::from(&*args);

    // Now fold the arguments in with the CLI config (which may fail)
    // (to provoke an error here: `qcp host: host2:`)
    let mut config_manager = Manager::try_from(&*args)?;
    setup_colours(&config_manager, mode)?;

    handle_mode(mode, &mut config_manager, args.client_params)
}

fn parse_args() -> Result<Option<Box<CliArgs>>> {
    use clap::error::ErrorKind::{DisplayHelp, DisplayVersion};
    match CliArgs::custom_parse(std::env::args_os()) {
        Ok(args) => Ok(Some(Box::new(args))),
        Err(e) if matches!(e.kind(), DisplayHelp | DisplayVersion) => {
            let message = e.render();
            if use_colours() {
                message.ansi().output_paged();
            } else {
                message.output_paged();
            }
            Ok(None)
        }
        Err(e) => Err(e.into()),
    }
}

fn setup_colours(manager: &Manager, mode: MainMode) -> Result<()> {
    let colour_mode = match manager.get_color(Some(Configuration::system_default().color)) {
        Ok(c) => Some(c),
        // If the config file is invalid, and we're in server mode, we should not report an error here (that will confuse the remote, which is expecting a protocol banner).
        // Instead fall back to default; we'll send Negotiation Failed later after trying to unpick the config.
        Err(_) if mode == MainMode::Server => None,
        Err(e) => return Err(e.into()),
    };
    configure_colours(colour_mode);
    Ok(())
}

// MODE HANDLERS ///////////////////////////////////////////////////////////

#[tokio::main(flavor = "current_thread")]
async fn handle_mode(
    mode: MainMode,
    config_manager: &mut Manager,
    client_params: Parameters,
) -> Result<bool> {
    match mode {
        MainMode::HelpBuffers => Ok(print_help_buffers()),
        MainMode::ShowConfigFiles => {
            println!("{:?}", Manager::config_files());
            Ok(true)
        }
        MainMode::ShowConfig => show_config(config_manager),
        MainMode::Server => run_server().await,
        MainMode::Client => run_client(config_manager, client_params).await,
        MainMode::ListFeatures => Ok(list_features()),
    }
}

fn list_features() -> bool {
    use tabled::settings::{Alignment, object::Columns};

    let mut tbl = crate::protocol::compat::pretty_list();
    let _ = tbl
        .with(crate::cli::styles::TABLE_STYLE.clone())
        .modify(Columns::last(), Alignment::center());
    format!("{tbl}").output_paged();
    true
}

fn print_help_buffers() -> bool {
    let _ = writeln!(
        std::io::stdout(),
        "{}",
        os::Platform::help_buffers_mode(Configuration::recv_buffer(), Configuration::send_buffer(),)
    );
    true
}

fn show_config(config_manager: &mut Manager) -> Result<bool> {
    config_manager.apply_system_default();
    format!(
        "Client configuration:\n{}",
        config_manager.to_display_adapter::<Configuration>()
    )
    .output_paged();
    config_manager.validate_configuration()?;
    Ok(true)
}

async fn run_server() -> Result<bool> {
    crate::server_main().await.map_err(|e| {
        eprintln!("{ERROR}ERROR{RESET} Server: {e:?}", ERROR = error());
        anyhow::anyhow!("Server failed")
    })?;
    Ok(true)
}

async fn run_client(config_manager: &mut Manager, client_params: Parameters) -> Result<bool> {
    let progress =
        MultiProgress::with_draw_target(ProgressDrawTarget::stderr_with_hz(MAX_UPDATE_FPS));
    {
        // Caution: We haven't applied the system default config at this point, so we don't necessarily have all the fields.
        // In order to validate what we have, we need to temporarily underlay the system default.
        let mut temp_mgr = config_manager.clone();
        temp_mgr.apply_system_default();
        temp_mgr.validate_configuration()?;
    }

    // this mode may return false
    crate::client_main(config_manager, progress, client_params).await
}
