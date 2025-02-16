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

/// Error message styling. This can be Displayed directly.
pub const ERROR: anstyle::Style = anstyle::Style::new().bold().fg_color(Some(Ansi(Red)));
/// Warning message styling. This can be Displayed directly.
pub const WARNING: anstyle::Style = anstyle::Style::new().bold().fg_color(Some(Ansi(Yellow)));
/// Informational message styling. This can be Displayed directly.
pub const INFO: anstyle::Style = anstyle::Style::new().fg_color(Some(Ansi(Cyan)));
// pub(crate) const DEBUG: anstyle::Style = anstyle::Style::new().fg_color(Some(Ansi(Blue)));
/// Success message style. This can be Displayed directly.
pub const SUCCESS: anstyle::Style = anstyle::Style::new().fg_color(Some(Ansi(Green)));

pub(crate) const HEADER: anstyle::Style = anstyle::Style::new()
    .underline()
    .fg_color(Some(Ansi(Yellow)));

pub(crate) const CLAP_STYLES: Styles = Styles::styled()
    .usage(HEADER)
    .header(HEADER)
    .literal(anstyle::Style::new().bold())
    .invalid(WARNING)
    .error(ERROR)
    .valid(INFO.bold().underline())
    .placeholder(INFO);

/// Resets styling to default.
///
/// This is purely for convenience; you can also call `ERROR::render_reset()` (etc.)
///
pub use anstyle::Reset as RESET;
