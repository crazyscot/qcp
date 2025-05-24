// (c) 2025 Ross Younger

use qcp::Configuration;
use qcp::config::{ColourMode, Manager};
use rusty_fork::rusty_fork_test;

fn test_body<T: FnOnce()>(setter: T, expected: ColourMode, check_meta: bool) {
    #[allow(unsafe_code)]
    unsafe {
        std::env::remove_var("NO_COLOR");
        std::env::remove_var("CLICOLOR");
        std::env::remove_var("CLICOLOR_FORCE");
        setter();
    }
    let mut mgr = Manager::new(None, true, false);
    mgr.apply_system_default();
    let result = mgr.get::<Configuration>().unwrap();
    assert_eq!(result.color, expected.into());

    if check_meta {
        let val = mgr.data_().find_value("color").unwrap();
        let meta = mgr.data_().get_metadata(val.tag()).unwrap();
        assert!(meta.name.contains("environment"));
    }
}

macro_rules! test_case {
    ($name:ident, $var:literal, $val:literal, $expected:ident, $check_meta:expr) => {
        rusty_fork_test! {
            #[test]
            fn $name() {
                test_body(
                    || {
                        #[allow(unsafe_code)]
                        unsafe {
                            std::env::set_var($var, $val);
                        }
                    },
                    ColourMode::$expected,
                    $check_meta
                );
            }
        }
    };
}

test_case!(force, "CLICOLOR_FORCE", "1", Always, true);
test_case!(auto, "CLICOLOR", "1", Auto, true);
test_case!(off, "CLICOLOR", "0", Never, true);
test_case!(no, "NO_COLOR", "1", Never, true);
test_case!(nothing, "nothing", "no", Auto, false);
