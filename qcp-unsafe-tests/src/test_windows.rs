// (c) 2025 Ross Younger

use qcp::os::{AbstractPlatform as _, WindowsPlatform as Platform};
use rusty_fork::rusty_fork_test;
use static_str_ops::static_format;
use std::path::MAIN_SEPARATOR;

rusty_fork_test! {
    #[test]
    fn config_paths() {
        // It's tricky to assert much about these paths as we might be running on CI (Linux) or on Windows.
        // So we use rusty_fork_test, and on Unix we setenv to let them work.
        if cfg!(unix) {
            #[allow(unsafe_code)]
            unsafe {
                // SAFETY: This is run in single threaded mode.
                std::env::set_var("ProgramData", static_format!("{MAIN_SEPARATOR}programdata"));
                std::env::set_var("UserProfile", static_format!("{MAIN_SEPARATOR}userprofile"));
            }
        }

        assert!(Platform::system_ssh_config().unwrap().to_string_lossy().ends_with(static_format!("{MAIN_SEPARATOR}ssh_config")));
        assert!(Platform::user_ssh_config().unwrap().to_string_lossy().ends_with(static_format!("{MAIN_SEPARATOR}config")));
        assert!(Platform::system_config_path().unwrap().to_string_lossy().ends_with(static_format!("{MAIN_SEPARATOR}qcp.conf")));

        let pv = Platform::user_config_paths();
        assert!(pv.len() == 1);
    }
}
