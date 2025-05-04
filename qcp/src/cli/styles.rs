// (c) 2024 Ross Younger
//! CLI output styling
//!
//! Users of this module probably ought to use anstream's `println!` / `eprintln!` macros.
//!
//! This module provides styles for use with those macros, and also a `RESET` constant to reset
//! styling to the default.

#[allow(clippy::enum_glob_use)]
use anstyle::AnsiColor::*;
use anstyle::Color::Ansi;
use clap::builder::styling::Styles;
use serde::Serialize;
use std::sync::{LazyLock, atomic::AtomicBool};

use crate::util::enums::ConvertibleTo;

// RAW STYLE DEFINITIONS //////////////////////////////////////////////////////////////////

/// Error message styling. This can be Displayed directly.
const _ERROR: anstyle::Style = anstyle::Style::new().bold().fg_color(Some(Ansi(Red)));

/// Warning message styling. This can be Displayed directly.
const _WARNING: anstyle::Style = anstyle::Style::new().bold().fg_color(Some(Ansi(Yellow)));

/// Informational message styling. This can be Displayed directly.
const _INFO: anstyle::Style = anstyle::Style::new().fg_color(Some(Ansi(Cyan)));

// pub(crate) const DEBUG: anstyle::Style = anstyle::Style::new().fg_color(Some(Ansi(Blue)));

/// Success message style. This can be Displayed directly.
const _SUCCESS: anstyle::Style = anstyle::Style::new().fg_color(Some(Ansi(Green)));

const _HEADER: anstyle::Style = anstyle::Style::new()
    .underline()
    .fg_color(Some(Ansi(Yellow)));

/// Resets styling to default.
///
/// This is purely for convenience; you can also call `error()::render_reset()` (etc.)
///
pub(crate) use anstyle::Reset as RESET;

// COMPOSITE STYLES //////////////////////////////////////////////////////////////////////

// We don't need to make this conditional, as clap already reads the CLICOLOR environment variables.
pub(crate) const CLAP_STYLES: Styles = Styles::styled()
    .usage(_HEADER)
    .header(_HEADER)
    .literal(anstyle::Style::new().bold())
    .invalid(_WARNING)
    .error(_ERROR)
    .valid(_INFO.bold().underline())
    .placeholder(_INFO);

// CONDITIONAL STYLES ////////////////////////////////////////////////////////////////////

/// Wrap a constant in a function that returns the style if colours are enabled.
macro_rules! wrap {
    ($func:ident, $def:ident) => {
        #[allow(clippy::missing_const_for_fn)]
        pub(crate) fn $func() -> anstyle::Style {
            if use_colours() {
                $def
            } else {
                anstyle::Style::new()
            }
        }
    };
}

wrap!(error, _ERROR);
wrap!(warning, _WARNING);
wrap!(info, _INFO);
wrap!(success, _SUCCESS);
wrap!(header, _HEADER);

// CONDITIONALITY & CLI //////////////////////////////////////////////////////////

static COLOURS_ENABLED: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(false));

pub(crate) fn use_colours() -> bool {
    COLOURS_ENABLED.load(std::sync::atomic::Ordering::Relaxed)
}

/// The available terminal colour modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")] // to match clap::ValueEnum
pub enum ColourMode {
    #[value(alias = "on", alias = "yes")]
    /// Forces colours on, whatever is happening
    /// (aliases: `on`, `yes`)
    Always,
    #[value(alias = "off", alias = "no", alias = "none")]
    /// (aliases: `off`, `no`, `none`)
    Never,
    /// Use colours only when writing to a terminal
    Auto,
}

/// Set up the terminal colour mode for this crate only, without setting environment variables.
///
/// This allows us to respect the settings as far as possible, before we have parsed
/// the configuration file and CLI arguments.
///
/// This function may safely be called multiple times but should not be called after calling
/// `configure_colours()`.
pub(crate) fn configure_colours_preliminary(mode: Option<ColourMode>) {
    let enable = match mode {
        Some(ColourMode::Always) => true,
        Some(ColourMode::Never) => false,
        Some(ColourMode::Auto) | None => clicolors_control::colors_enabled(),
    };
    COLOURS_ENABLED.store(enable, std::sync::atomic::Ordering::Relaxed);
}

/// Set up the terminal colour mode.
/// If `mode` is `None`, we will use the standard `CLICOLOR` environment variables to determine the mode.
/// See [https://bixense.com/clicolors/](https://bixense.com/clicolors/) for more information.
///
/// This function should only be called once, at the start of the program.
///
/// SAFETY: This function sets environment variables, which is not thread-safe on all platforms.
/// Justification: There is no way to tell all the downstream crates to use colours (or not) other than the environment.
#[allow(unsafe_code)]
pub(crate) unsafe fn configure_colours<CM>(mode: CM)
where
    CM: ConvertibleTo<Option<ColourMode>>,
{
    let mode = match mode.convert() {
        Some(m) => m,
        None => {
            // fall back to env vars & autodetection
            if clicolors_control::colors_enabled() {
                ColourMode::Always
            } else {
                ColourMode::Never
            }
        }
    };

    match mode {
        ColourMode::Always => {
            clicolors_control::set_colors_enabled(true);
            #[allow(unsafe_code)]
            unsafe {
                std::env::set_var("CLICOLOR_FORCE", "1");
            }
            COLOURS_ENABLED.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        ColourMode::Never => {
            clicolors_control::set_colors_enabled(false);
            #[allow(unsafe_code)]
            unsafe {
                std::env::remove_var("CLICOLOR_FORCE");
                std::env::set_var("CLICOLOR", "0");
            }
            COLOURS_ENABLED.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        ColourMode::Auto => {
            #[allow(unsafe_code)]
            unsafe {
                std::env::remove_var("CLICOLOR_FORCE");
                std::env::set_var("CLICOLOR", "1");
            }
            COLOURS_ENABLED.store(
                clicolors_control::colors_enabled(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }
}
