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
use std::io::IsTerminal;
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
        #[allow(dead_code)] // not all of these functions are used on all platforms
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

static COLOURS_ENABLED: LazyLock<AtomicBool> =
    LazyLock::new(|| AtomicBool::new(autodetect_colour()));

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
/// If `mode` is `None`, we will use the quasi-standard `CLICOLOR`, `CLICOLOR_FORCE` and `NO_COLOR` environment variables to determine the mode.
/// See [https://bixense.com/clicolors/](https://bixense.com/clicolors/) for more information.
///
/// This function should only be called once, at the start of the program.
///
/// SAFETY: This function sets environment variables, which is not thread-safe on all platforms.
/// Justification: There is no way to tell all the downstream crates to use colours (or not) other than the environment.
#[allow(unsafe_code)]
pub(crate) unsafe fn configure_colours<CM>(mode: CM)
where
    CM: ConvertibleTo<Option<ColourMode>> + std::fmt::Debug,
{
    let state = match mode.convert() {
        Some(ColourMode::Always) => true,
        Some(ColourMode::Never) => false,
        None | Some(ColourMode::Auto) => autodetect_colour(),
    };
    COLOURS_ENABLED.store(state, std::sync::atomic::Ordering::Relaxed);
    #[allow(unsafe_code)]
    unsafe {
        if state {
            std::env::set_var("CLICOLOR_FORCE", "1");
            std::env::set_var("CLICOLOR", "1");
            std::env::remove_var("NO_COLOR");
        } else {
            std::env::remove_var("CLICOLOR_FORCE");
            std::env::set_var("CLICOLOR", "0");
            std::env::set_var("NO_COLOR", "1");
        }
    }
}

// ///////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::{ColourMode, configure_colours, use_colours};
    use rusty_fork::rusty_fork_test;
    use std::{collections::HashMap, env};

    const VARS_TO_CLEAN: &[&str] = &["CLICOLOR", "CLICOLOR_FORCE", "NO_COLOR"];

    fn test_case(map: &HashMap<&str, &str>, setting: Option<ColourMode>, enabled: bool) {
        #[allow(unsafe_code)]
        unsafe {
            VARS_TO_CLEAN.iter().for_each(|v| env::remove_var(v));
            for (var, value) in map {
                if value.is_empty() {
                    env::remove_var(var);
                } else {
                    env::set_var(var, value);
                }
            }
            configure_colours(setting);
        }
        assert!(use_colours() == enabled);
        if enabled {
            assert_eq!(super::error(), super::_ERROR);
            assert_eq!(env::var("CLICOLOR_FORCE").unwrap_or_default(), "1");
            assert_eq!(
                env::var("NO_COLOR").unwrap_err().to_string(),
                "environment variable not found"
            );
        } else {
            assert_eq!(super::error(), anstyle::Style::new());
            assert_eq!(env::var("CLICOLOR").unwrap_or_default(), "0");
            assert_eq!(
                env::var("CLICOLOR_FORCE").unwrap_err().to_string(),
                "environment variable not found"
            );
            assert!(env::var("NO_COLOR").is_ok());
        }
    }

    // TODO: I haven't yet figured out how to make rstest play nicely with rusty_fork
    macro_rules! test_case {
        ($name:ident, $env_vars:expr, $setting:expr, $expected:expr) => {
            // these tests affect global state, so need to run in forks
            rusty_fork_test! {
                #[test]
                fn $name() {
                    let map = HashMap::from($env_vars);
                    test_case(&map, $setting, $expected);
                }
            }
        };
    }

    test_case!(off, [], Some(ColourMode::Never), false);
    test_case!(on, [], Some(ColourMode::Always), true);
    test_case!(
        auto_on,
        [("CLICOLOR_FORCE", "1")],
        Some(ColourMode::Auto),
        true
    );
    test_case!(none_on, [("CLICOLOR_FORCE", "1")], None, true);
    test_case!(auto_off, [("CLICOLOR", "0")], Some(ColourMode::Auto), false);
    test_case!(none_off, [("CLICOLOR", "0")], None, false);
    test_case!(never, [("NO_COLOR", "1")], Some(ColourMode::Never), false);
}
