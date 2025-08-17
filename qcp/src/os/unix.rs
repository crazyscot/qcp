// (c) 2024 Ross Younger
// Code in this module: OS concretions for Unix platforms
// Docs in this module: Linux/Unix notes
//
//! 🐧 qcp on Linux and other Unix platforms
//!
//! (See also [OSX](super::osx) specific notes.)
//!
//! ## Getting started (IMPORTANT)
//!
//! **Short version:** Run `qcp --help-buffers` and follow its instructions.
//!
//! *Long version:*
//!
//! The Linux kernel, by default, sets quite a low limit for UDP send and receive buffers.
//!
//! 🚀 **It is essential to change this setting for good performance.**
//!
//! There is a runtime check that warns of any detected kernel configuration issues and advises how the sysadmin can resolve these.
//!
//! The Debian/Ubuntu packaging drops a file into `/etc/sysctl.d/20-qcp.conf` that does this for you.
//!
//! ## Packaging
//!
//! The released Linux binary images (named `qcp-linux-<ARCH>-musl.tar.gz`) should work with any
//! recent Linux kernel, independent of distribution.
//! (They are statically linked against `musl`.)

use crate::cli::styles::{RESET, error, header, info, success, warning};
use crate::config::BASE_CONFIG_FILENAME;

use human_repr::HumanCount as _;
use rustix::process::{Uid, geteuid};

use std::path::PathBuf;

/// Unix platform implementation (Linux, OSX, BSD and others)
#[allow(missing_copy_implementations, missing_debug_implementations)]
pub struct UnixPlatform {}

impl super::AbstractPlatform for UnixPlatform {
    /// Location of the system ssh config file.
    fn system_ssh_dir_path() -> Option<PathBuf> {
        Some("/etc/ssh".into())
    }

    fn system_ssh_config() -> Option<PathBuf> {
        Some("/etc/ssh/ssh_config".into())
    }

    fn user_ssh_config() -> Option<PathBuf> {
        let mut pb = dirs::home_dir()?;
        pb.push(".ssh");
        pb.push("config");
        Some(pb)
    }

    fn user_config_path_extra() -> Option<PathBuf> {
        // ~/.qcp.conf for now
        let mut d: PathBuf = dirs::home_dir()?;
        d.push(format!(".{BASE_CONFIG_FILENAME}"));
        Some(d)
    }

    fn system_config_path() -> Option<PathBuf> {
        // /etc/<filename> for now
        let mut p: PathBuf = PathBuf::new();
        p.push("/etc");
        p.push(BASE_CONFIG_FILENAME);
        Some(p)
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn help_buffers_mode(udp: u64) -> String {
        help_buffers_unix(udp)
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn help_buffers_unix(udp: u64) -> String {
    #![allow(non_snake_case)]
    use std::fmt::Write as _;
    let INFO = info();
    let WARNING = warning();
    let HEADER = header();

    let mut output = String::from(super::TESTING_BUFFERS_MESSAGE);

    let result = super::test_udp_buffers(udp, udp);
    let tested_ok = match result {
        Err(e) => {
            let _ = write!(
                output,
                r"
⚠️ {WARNING}Unable to test local UdpSocket parameters:{RESET} {e}
Outputting general advice..."
            );
            false
        }
        Ok(r) => {
            let _ = write!(
                output,
                "\n{INFO}Test result:{RESET} {rx} (read) / {tx} (write)",
                rx = r.recv.human_count_bytes(),
                tx = r.send.human_count_bytes()
            );
            if let Some(warning) = r.warning {
                let _ = write!(output, "\n😞 {}{warning}{RESET}", error());
            } else {
                let _ = write!(output, "\n🚀 {SUCCESS}Great!{RESET}", SUCCESS = success());
            }
            r.ok
        }
    };

    if tested_ok {
        // root can usually override resource limits, so beware of false negatives
        if geteuid() != Uid::ROOT {
            let _ = write!(
                output,
                "💡 No administrative action is necessary. Happy qcp'ing!"
            );
            return output;
        }

        let _ = write!(
            output,
            r"

⚠️  {WARNING}CAUTION: This process is running as user id 0 (root).{RESET}
This isn't a problem, but it means we can't tell whether ordinary users are
able to set suitable UDP buffer limits. If you like, rerun as a non-root user.
For now, outputting general advice.
"
        );
    }

    if cfg!(linux) {
        let settings_file_msg = if std::fs::exists("/etc/sysctl.d/20-qcp.conf").unwrap_or(false) {
            "Check your settings in"
        } else {
            "To have this setting apply at boot, put this into"
        };
        let _ = write!(
            output,
            r"

🛠️  {settings_file_msg} {HEADER}/etc/sysctl.d/20-qcp.conf{RESET}:
    {INFO}net.core.rmem_max={udp}
    {INFO}net.core.wmem_max={udp}{RESET}

🛠️  To set the kernel limits immediately, run the following command as root:
    {INFO}sysctl -w net.core.rmem_max={udp} -w net.core.wmem_max={udp}{RESET}"
        );
    } else if cfg!(bsdish) {
        // Received wisdom about BSD kernels leads me to recommend 115% of the max. I'm not sure this is necessary.
        let size = udp * 115 / 100;
        let _ = write!(
            output,
            r"
🛠️ To set the kernel limits immediately, run the following command as root:
    {INFO}sysctl -w kern.ipc.maxsockbuf={size}{RESET}

To have this setting apply at boot, add this line to {HEADER}/etc/sysctl.conf{RESET}:
    {INFO}kern.ipc.maxsockbuf={size}{RESET}"
        );
    } else {
        let _ = write!(
            output,
            r"
{ERROR}Unknown unix build type!{RESET}

This build of qcp is configured for OS type '{os}',
which is not covered by any of the current help messages.
Sorry about that! If you can fill in the details, please get in touch with
the developers.

We need the kernel UDP buffer size limit to be at least {udp} for reading and writing
(assuming you want to work bidirectionally).

There might be a sysctl or similar mechanism you can set to enable this.
Good luck!",
            os = std::env::consts::OS,
            ERROR = error(),
        );
    }
    output
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::UnixPlatform as Platform;
    use crate::os::AbstractPlatform;

    #[cfg(target_os = "macos")]
    const HOME_COMMON: &str = "/Users/";
    #[cfg(not(target_os = "macos"))]
    const HOME_COMMON: &str = "/home/";

    #[test]
    fn config_paths() {
        assert!(
            Platform::system_ssh_config()
                .unwrap()
                .to_string_lossy()
                .contains("/etc/ssh/ssh_config")
        );
        let s = Platform::user_ssh_config().unwrap();
        assert!(s.to_string_lossy().contains(HOME_COMMON));
        let q = Platform::system_config_path().unwrap();
        assert!(q.to_string_lossy().starts_with("/etc/"));

        let pv = Platform::user_config_paths();
        eprintln!("{pv:?}");
        assert!(pv.len() == 2);
        assert!(pv[0].to_string_lossy().contains(HOME_COMMON));
        assert!(pv[0].to_string_lossy().contains("/qcp/qcp.conf"));
        assert!(pv[1].to_string_lossy().contains(HOME_COMMON));
        assert!(pv[1].to_string_lossy().contains("/.qcp.conf"));
    }
}
