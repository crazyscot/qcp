//! Main CLI for qcp
// (c) 2024 Ross Younger

use std::process::ExitCode;
use std::{ffi::OsString, io::Write as _};

use super::args::{CliArgs, MainMode};
use crate::{
    cli::styles::{configure_colours, error, reset, use_colours},
    client::MAX_UPDATE_FPS,
    config::{Configuration, Manager},
    os::{self, AbstractPlatform as _},
};

use anyhow::{Context, Result};
use indicatif::{MultiProgress, ProgressDrawTarget};
use lessify::OutputPaged;

/// Main CLI entrypoint
///
/// Call this from `main`, passing the arguments to use.
/// Normally you will call `cli(std::env::args_os())` but you can pass in alternate arguments for CLI testing.
///
/// # Safety
/// - This function may start a tokio runtime and perform work in it.
/// - This function is not safe to call from multi-threaded code.
#[must_use]
pub fn cli<I, T>(args: I) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    #[allow(clippy::match_bool)] // improved readability
    cli_inner(args)
        .inspect_err(|e| {
            if crate::util::tracing_is_initialised() {
                tracing::error!("{e:#}");
            } else {
                format!(
                    "{ERROR}Error:{RESET} {e:#}",
                    ERROR = error(),
                    RESET = reset()
                )
                .output_paged();
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
fn cli_inner<I, T>(args: I) -> Result<bool>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let Some(args) = parse_args(args)? else {
        return Ok(true); // help/version shown; exit
    };

    // Now fold the arguments in with the CLI config (which may fail)
    // (to provoke an error here: `qcp host: host2:`)
    let config_manager = Manager::try_from(&*args)?;
    setup_colours(&config_manager, args.mode_)?;

    handle_mode(args, config_manager)
}

fn parse_args<I, T>(args: I) -> Result<Option<Box<CliArgs>>>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    use clap::error::ErrorKind::{DisplayHelp, DisplayVersion};
    match CliArgs::custom_parse(args) {
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
async fn handle_mode(args: Box<CliArgs>, config_manager: Manager) -> Result<bool> {
    match args.mode_ {
        MainMode::HelpBuffers => print_help_buffers(config_manager),
        MainMode::ShowConfigFiles => {
            println!("{:?}", Manager::config_files());
            Ok(true)
        }
        MainMode::ShowConfig => show_config(config_manager),
        MainMode::Server => run_server().await,
        MainMode::Client => run_client(config_manager, args).await,
        MainMode::ListFeatures => Ok(list_features()),
    }
}

fn list_features() -> bool {
    list_features_data().output_paged();
    true
}
fn list_features_data() -> String {
    use tabled::settings::{Alignment, Width, object::Column};

    let terminal_width = match termsize::get() {
        None => 80,
        Some(termsize::Size { rows: _, cols: c }) => c,
    } as usize;

    let mut tbl = crate::protocol::compat::pretty_list();
    let mut longest_name = 0;
    for line in tbl.get_records().iter() {
        longest_name = longest_name.max(
            line.first()
                .map(tabled::grid::records::vec_records::Cell::width)
                .unwrap_or_default(),
        );
    }

    // there must be a nicer way to do this?
    // | <longest feature name> | Level | <AVAILABLE SPACE> |
    // => space for notes column = terminal_width - 15 - (length of longest feature name)

    let last_column_avail = terminal_width - 15 - longest_name;

    let _ = tbl
        .with(crate::cli::styles::TABLE_STYLE.clone())
        .modify(Column::from(1), Alignment::center());
    if last_column_avail > 10 {
        let _ = tbl.modify(
            Column::from(2),
            Width::wrap(last_column_avail).keep_words(true),
        );
    } // else it's going to look terrible, whatever we do
    format!("{tbl}")
}

fn print_help_buffers(manager: Manager) -> Result<bool> {
    let _ = writeln!(std::io::stdout(), "{}", help_buffers_data(manager)?);
    Ok(true)
}
fn help_buffers_data(mut manager: Manager) -> Result<String> {
    manager.apply_system_default();
    manager.validate_configuration()?;
    let config = manager.get::<Configuration>()?;
    Ok(os::Platform::help_buffers_mode(config.udp_buffer))
}

fn show_config(mut config_manager: Manager) -> Result<bool> {
    show_config_data(&mut config_manager).output_paged();
    config_manager.validate_configuration()?;
    Ok(true)
}
fn show_config_data(config_manager: &mut Manager) -> String {
    config_manager.apply_system_default();
    format!(
        "Client configuration:\n{}",
        config_manager.to_display_adapter::<Configuration>()
    )
}

async fn run_server() -> Result<bool> {
    Ok(crate::server_main()
        .await
        .with_context(|| "ERROR [Server] main loop failed")
        .inspect_err(|e| eprintln!("{e:?}")) // it's critical that we output to stderr, so the message is not interpreted as protocol data
        .is_ok())
}

async fn run_client(config_manager: Manager, args: Box<CliArgs>) -> Result<bool> {
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
    crate::client_main(config_manager, progress, args).await
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use assertables::assert_contains;

    use super::MainMode;
    use crate::{
        cli::{
            CliArgs,
            cli_main::{help_buffers_data, list_features_data, show_config_data},
        },
        config::Manager,
    };

    fn test_mgr() -> Manager {
        let mut mgr = Manager::without_default(None);
        mgr.apply_system_default();
        mgr
    }

    #[test]
    fn help_buffers() {
        assert_contains!(
            help_buffers_data(test_mgr()).unwrap(),
            "Testing this system"
        );
    }

    #[test]
    fn list_features() {
        let data = list_features_data();
        assert_contains!(data, "Feature");
        assert_contains!(data, "BasicProtocol");
    }

    #[test]
    fn show_config() {
        let mut mgr = test_mgr();
        let data = show_config_data(&mut mgr);
        assert_contains!(data, "Client config");
        assert_contains!(data, "Remote host");
        assert_contains!(data, "AddressFamily");
    }
    #[test]
    fn show_config_files_requires_no_paths() {
        let args = &["qcp", "--config-files"];
        let parsed = CliArgs::custom_parse(args).unwrap();
        let mgr = Manager::standard(None);
        assert!(crate::cli::cli_main::handle_mode(Box::new(parsed), mgr).unwrap());
    }

    #[test]
    fn setup_colours_invalid_config() {
        let mgr = littertray::LitterTray::try_with(|tray| {
            let path = "test.conf";
            let _ = tray.create_text(
                path,
                r"
            Host *
            color invalid
        ",
            )?;
            let mut mgr = Manager::without_files(None);
            mgr.merge_ssh_config(path, None, false);
            Ok(mgr)
        })
        .unwrap();

        super::setup_colours(&mgr, MainMode::Server).unwrap();
        let e = super::setup_colours(&mgr, MainMode::Client).unwrap_err();
        eprintln!("{e}");
    }
}
