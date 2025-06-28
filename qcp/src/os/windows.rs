// (c) 2025 Ross Younger
// Code in this module: OS concretions for Windows
//
//! 🪟 qcp on Windows
//!
//! ## 🖥 Server Mode
//!
//! #### 1. Get ssh going first
//!
//! I tested on Windows 11 with OpenSSH Server (installed via System -> Optional features).
//! See <https://learn.microsoft.com/en-us/windows-server/administration/OpenSSH/openssh-server-configuration> for details.
//!
//! I found I needed to manually start sshd (in Services).
//!
//! I found I needed to explicitly allow access through the Windows firewall for sshd before I could connect to it.
//!
//! #### 2. Set up qcp somewhere the ssh daemon can reach it
//!
//! You need to put the qcp executable somewhere that the ssh server can find it on a non-interactive login.
//! The most convenient place I found was in my profile directory `C:\Users\myusername`.
//!
//! ssh subsystem mode may be an option, but I couldn't readily get that working.
//!
//! #### 3. Allow access through the Windows firewall
//!
//! You need to allow qcp through the Windows firewall.
//! If you don't, the machine initiating the connection will complain of a protocol
//! timeout setting up the QUIC session.
//!
//! This is in Windows Security > Firewall & network protection > Allow an app through firewall.
//!
//! # 🚀 Network tuning
//!
//! qcp performed well straight out of the box; no additional system configuration was necessary.
//! Nevertheless, we still check the buffer sizes at runtime, to be able to warn if this isn't the case in future.
//!
//! # 🕵️ Troubleshooting
//!
//! #### Protocol timeout setting up the QUIC session
//!
//! Most likely the Windows firewall, or some other intervening firewall, is blocking the
//! UDP packets. [See above](#3-allow-access-through-the-windows-firewall).

use crate::cli::styles::{RESET, info, success, warning};
use crate::config::BASE_CONFIG_FILENAME;

use human_repr::HumanCount as _;
use std::path::PathBuf;

/// Windows platform implementation
#[allow(missing_copy_implementations, missing_debug_implementations)]
pub struct WindowsPlatform {}

impl super::AbstractPlatform for WindowsPlatform {
    /// System ssh config file. On Windows this is `%ProgramData%\ssh\ssh_config`
    fn system_ssh_config() -> Option<PathBuf> {
        let Ok(progdata) = std::env::var("ProgramData") else {
            return None;
        };
        let mut pb = PathBuf::new();
        pb.push(progdata);
        pb.push("ssh");
        pb.push("ssh_config");
        Some(pb)
    }

    /// User config file. On Windows this is `%UserProfile%\.ssh\config`
    fn user_ssh_config() -> Option<PathBuf> {
        let Ok(pd) = std::env::var("UserProfile") else {
            return None;
        };
        let mut pb = PathBuf::new();
        pb.push(pd);
        pb.push(".ssh");
        pb.push("config");
        Some(pb)
    }

    /// No extra user configuration file path is provided on Windows.
    fn user_config_path_extra() -> Option<PathBuf> {
        None
    }

    /// Path to the system config file. On Windows this is `%ProgramData%\qcp.conf`
    fn system_config_path() -> Option<PathBuf> {
        let Ok(progdata) = std::env::var("ProgramData") else {
            return None;
        };
        let mut p: PathBuf = PathBuf::from(progdata);
        p.push("qcp");
        p.push(BASE_CONFIG_FILENAME);
        Some(p)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn help_buffers_mode(udp: u64) -> String {
        help_buffers_win(udp)
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn help_buffers_win(udp: u64) -> String {
    #![allow(non_snake_case)]
    let result = super::test_udp_buffers(udp, udp);
    match result {
        Err(e) => {
            format!(
                r"⚠️ {WARNING}Unable to test local UdpSocket parameters:{RESET} {e}
                Sorry, we can't predict performance. Proceed with caution.",
                WARNING = warning()
            )
        }
        Ok(r) => {
            use std::fmt::Write as _;
            let mut output = format!(
                "\n{INFO}Test result:{RESET} {rx} (read) / {tx} (write)",
                rx = r.recv.human_count_bytes(),
                tx = r.send.human_count_bytes(),
                INFO = info(),
            );
            if let Some(warning) = r.warning {
                let _ = write!(output, "\n😞 {INFO}{warning}{RESET}", INFO = info());
            } else {
                let _ = write!(output, "\n🚀 {SUCCESS}Great!{RESET}", SUCCESS = success());
            }
            output
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
/// See also [`qcp_unsafe_tests::test_windows`]
mod test {
    use super::WindowsPlatform as Platform;
    use crate::os::AbstractPlatform;

    #[cfg(unix)]
    #[test]
    fn config_paths_unset() {
        assert!(Platform::system_ssh_config().is_none());
        assert!(Platform::user_ssh_config().is_none());
        assert!(Platform::system_config_path().is_none());
    }
}
