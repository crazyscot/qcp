// (c) 2025 Ross Younger

use anstream::ColorChoice;
use qcp::config::ColourMode;
use qcp::styles::{configure_colours, error, use_colours};
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
    assert_eq!(use_colours(), enabled);
    assert_eq!(console::colors_enabled(), enabled);
    assert_eq!(console::colors_enabled_stderr(), enabled);
    let choice = ColorChoice::global();
    if enabled {
        assert_eq!(choice, ColorChoice::Always);
        assert_ne!(error(), anstyle::Style::new());
    } else {
        assert_eq!(choice, ColorChoice::Never);
        assert_eq!(error(), anstyle::Style::new());
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
