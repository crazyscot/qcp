// (c) 2024 Ross Younger
//! CLI output styling
//!
//! Users of this module probably ought to use anstream's `println!` / `eprintln!` macros,

#[allow(clippy::enum_glob_use)]
use anstyle::AnsiColor::*;
use anstyle::Color::Ansi;
use anstyle_owo_colors::to_owo_style;
use clap::builder::styling::Styles;
use lazy_static::lazy_static;
use owo_colors::Style as OwoStyle;

pub(crate) const ERROR: anstyle::Style = anstyle::Style::new().bold().fg_color(Some(Ansi(Red)));
pub(crate) const WARNING: anstyle::Style =
    anstyle::Style::new().bold().fg_color(Some(Ansi(Yellow)));
pub(crate) const INFO: anstyle::Style = anstyle::Style::new().fg_color(Some(Ansi(Cyan)));
pub(crate) const DEBUG: anstyle::Style = anstyle::Style::new().fg_color(Some(Ansi(Blue)));

lazy_static! {
    pub(crate) static ref ERROR_S: OwoStyle = to_owo_style(ERROR);
    pub(crate) static ref WARNING_S: OwoStyle = to_owo_style(WARNING);
    pub(crate) static ref INFO_S: OwoStyle = to_owo_style(INFO);
    pub(crate) static ref DEBUG_S: OwoStyle = to_owo_style(DEBUG);
}

pub(crate) const CALL_OUT: anstyle::Style = anstyle::Style::new()
    .underline()
    .fg_color(Some(Ansi(Yellow)));

pub(crate) const CLAP_STYLES: Styles = Styles::styled()
    .usage(CALL_OUT)
    .header(CALL_OUT)
    .literal(anstyle::Style::new().bold())
    .invalid(WARNING)
    .error(ERROR)
    .valid(INFO.bold().underline())
    .placeholder(INFO);
