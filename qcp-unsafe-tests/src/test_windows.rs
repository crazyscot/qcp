// (c) 2025 Ross Younger

use cfg_if::cfg_if;
use qcp::os::{AbstractPlatform as _, WindowsPlatform as Platform};
use rusty_fork::rusty_fork_test;

rusty_fork_test! {
    #[test]
    fn config_paths() {
        // It's tricky to assert much about these paths as we might be running on CI (Linux) or on Windows.
        // So we use rusty_fork_test, and on Unix we setenv to let them work.
        cfg_if! {
            if #[cfg(unix)] {
                #[allow(unsafe_code)]
                unsafe {
                    // SAFETY: This is run in single threaded mode.
                    std::env::set_var("ProgramData", "/programdata");
                    std::env::set_var("UserProfile", "/userprofile");
                }
            }
        }

        assert!(Platform::system_ssh_config().unwrap().to_string_lossy().ends_with("/ssh_config"));
        assert!(Platform::user_ssh_config().unwrap().to_string_lossy().ends_with("/config"));
        assert!(Platform::system_config_path().unwrap().to_string_lossy().ends_with("/qcp.conf"));

        let pv = Platform::user_config_paths();
        assert!(pv.len() == 1);
    }
}
