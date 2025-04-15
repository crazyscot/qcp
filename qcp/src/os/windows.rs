//! OS concretions for Windows
// (c) 2025 Ross Younger

use crate::cli::styles::{INFO, RESET, SUCCESS, WARNING};
use crate::config::BASE_CONFIG_FILENAME;

use human_repr::HumanCount as _;
use std::path::PathBuf;

/// Windows platform implementation
#[derive(Debug, Clone, Copy)]
pub struct Platform {}

impl super::AbstractPlatform for Platform {
    fn system_ssh_config() -> Option<PathBuf> {
        // %ProgramData%\ssh\ssh_config
        let Ok(progdata) = std::env::var("ProgramData") else {
            return None;
        };
        let mut pb = PathBuf::new();
        pb.push(progdata);
        pb.push("ssh");
        pb.push("ssh_config");
        Some(pb)
    }

    fn user_ssh_config() -> Option<PathBuf> {
        // %UserProfile%\.ssh\config
        let Ok(pd) = std::env::var("UserProfile") else {
            return None;
        };
        let mut pb = PathBuf::new();
        pb.push(pd);
        pb.push(".ssh");
        pb.push("config");
        Some(pb)
    }

    fn user_config_path_extra() -> Option<PathBuf> {
        None
    }

    fn system_config_path() -> Option<PathBuf> {
        // %ProgramData%\qcp.conf
        let Ok(progdata) = std::env::var("ProgramData") else {
            return None;
        };
        let mut p: PathBuf = PathBuf::from(progdata);
        p.push("qcp");
        p.push(BASE_CONFIG_FILENAME);
        Some(p)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn help_buffers_mode(rmem: u64, wmem: u64) {
        help_buffers_win(rmem, wmem);
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn help_buffers_win(rmem: u64, wmem: u64) {
    let result = super::test_buffers(rmem, wmem);
    let _tested_ok = match result {
        Err(e) => {
            println!(
                r"
⚠️ {WARNING}Unable to test local UdpSocket parameters:{RESET} {e}
   Sorry, we can't predict performance. Proceed with caution."
            );
            false
        }
        Ok(r) => {
            println!(
                "{INFO}Test result:{RESET} {rx} (read) / {tx} (write)",
                rx = r.recv.human_count_bytes(),
                tx = r.send.human_count_bytes()
            );
            if let Some(warning) = r.warning {
                println!(
                    r"
😞 {INFO}{warning}{RESET}"
                );
            } else {
                println!(
                    r"
🚀 {SUCCESS}Great!{RESET}"
                );
            }
            r.ok
        }
    };
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::super::test_buffers;
    use super::Platform;
    use crate::os::AbstractPlatform;
    use cfg_if::cfg_if;
    use rusty_fork::rusty_fork_test;

    rusty_fork_test! {
        #[test]
        fn config_paths() {
            // It's tricky to assert much about these paths as we might be running on CI (Linux) or on Windows.
            // So we use rusty_fork_test, and setenv.
            cfg_if! {
                if #[cfg(unix)] {
                    #[allow(unsafe_code)]
                    unsafe {
                        std::env::set_var("ProgramData", "/programdata");
                        std::env::set_var("UserProfile", "/userprofile");
                    }
                }
            }

            assert!(Platform::system_ssh_config().unwrap().to_string_lossy().contains("/ssh_config"));
            assert!(Platform::user_ssh_config().unwrap().to_string_lossy().contains("/config"));
            assert!(Platform::system_config_path().unwrap().to_string_lossy().contains("/qcp.conf"));

            let pv = Platform::user_config_paths();
            assert!(pv.len() == 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn config_paths_unset() {
        assert!(Platform::system_ssh_config().is_none());
        assert!(Platform::user_ssh_config().is_none());
        assert!(Platform::system_config_path().is_none());
    }

    #[test]
    fn test_buffers_small_ok() {
        assert!(test_buffers(131_072, 131_072).unwrap().warning.is_none());
    }
    #[test]
    fn test_buffers_gigantic_err() {
        assert!(
            test_buffers(1_073_741_824, 1_073_741_824)
                .unwrap()
                .warning
                .is_some()
        );
    }
}
