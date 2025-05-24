// (c) 2024 Ross Younger
//! CLI output styling
//!
//! This module provides styles for use with those macros, and also a `RESET` constant to reset
//! styling to the default.

use anstream::ColorChoice;
#[allow(clippy::enum_glob_use)]
use anstyle::AnsiColor::*;
use anstyle::Color::Ansi;
use clap::builder::styling::Styles;
use serde::Serialize;
use std::borrow::Cow;
use std::io::IsTerminal;

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

/// Resets styling to default. This is a re-export of [`anstyle::Reset`].
///
/// This is purely for convenience; you can also call `error()::render_reset()` (etc.)
///
pub use anstyle::Reset as RESET;

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
        #[allow(dead_code)] // not all of these functions are used on all platforms
        #[must_use]
        /// Conditional styling accessor for
        #[doc = stringify!($func)]
        /// messages
        ///
        /// This function returns either an active [`anstyle::Style`], or
        /// (if colours are disabled) the empty Style.
        pub fn $func() -> anstyle::Style {
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

/// Are we configured to use terminal colours?
#[must_use]
pub fn use_colours() -> bool {
    console::colors_enabled()
}

/// The available terminal colour modes
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Serialize, strum::VariantNames)]
#[serde(rename_all = "kebab-case")] // to match clap::ValueEnum
pub enum ColourMode {
    #[value(alias = "on", alias = "yes")]
    /// Forces colours on, whatever is happening
    /// (aliases: `on`, `yes`)
    Always,
    #[value(alias = "off", alias = "no", alias = "none")]
    /// (aliases: `off`, `no`, `none`)
    Never,
    /// Use colours only when writing to a terminal. This is the default behaviour.
    Auto,
}

/// Detect the desired colour mode from the environment variables
///
/// See [https://bixense.com/clicolors/](https://bixense.com/clicolors/) for more information.
pub(crate) fn autodetect_colour() -> bool {
    let clicolor_force = std::env::var("CLICOLOR_FORCE").unwrap_or_default();
    let no_color = std::env::var("NO_COLOR").unwrap_or_default();

    if !no_color.is_empty() {
        false
    } else if !clicolor_force.is_empty() {
        true
    } else {
        // This program chooses to default colours to ON, unless explicitly disabled, so reading CLICOLOR is unnecessary.
        std::io::stdout().is_terminal()
    }
}

/// Set up the terminal colour mode.
///
/// If `mode` is `None`, we will use the quasi-standard `CLICOLOR`, `CLICOLOR_FORCE` and `NO_COLOR` environment variables to determine the mode.
/// See [https://bixense.com/clicolors/](https://bixense.com/clicolors/) for more information.
pub fn configure_colours<CM>(mode: CM)
where
    CM: ConvertibleTo<Option<ColourMode>> + std::fmt::Debug,
{
    let state = match mode.convert() {
        Some(ColourMode::Always) => true,
        Some(ColourMode::Never) => false,
        None | Some(ColourMode::Auto) => autodetect_colour(),
    };
    console::set_colors_enabled(state);
    console::set_colors_enabled_stderr(state);
    if state {
        ColorChoice::Always
    } else {
        ColorChoice::Never
    }
    .write_global();
}

pub(crate) fn maybe_strip_color(s: &str) -> Cow<'_, str> {
    if use_colours() {
        s.into()
    } else {
        console::strip_ansi_codes(s)
    }
}

// Tests for this module are in `qcp_unsafe_tests::styles`.
