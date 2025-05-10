//! OS concretions for Unix platforms
// (c) 2024 Ross Younger

use crate::cli::styles::{RESET, error, header, info, success, warning};
use crate::config::BASE_CONFIG_FILENAME;

use human_repr::HumanCount as _;
use rustix::process::{Uid, geteuid};

use std::path::PathBuf;

/// Unix platform implementation
#[derive(Debug, Clone, Copy)]
pub struct Platform {}

impl super::AbstractPlatform for Platform {
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
    fn help_buffers_mode(rmem: u64, wmem: u64) -> String {
        help_buffers_unix(rmem, wmem)
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn help_buffers_unix(rmem: u64, wmem: u64) -> String {
    #![allow(non_snake_case)]
    use std::fmt::Write as _;
    let INFO = info();
    let WARNING = warning();
    let HEADER = header();

    let mut output = String::from(
        r"ℹ️  For best performance, it is necessary to set the kernel UDP buffer size limits.
This program attempts to automatically set buffer sizes for itself,
but this usually requires the kernel limits to be configured appropriately.

Testing this system...",
    );

    let result = super::test_buffers(rmem, wmem);
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
                let _ = write!(output, "\n😞 {INFO}{warning}{RESET}");
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
        let _ = write!(
            output,
            r"
🛠️  To set the kernel limits immediately, run the following command as root:
    {INFO}sysctl -w net.core.rmem_max={rmem} -w net.core.wmem_max={wmem}{RESET}

🛠️  To have this setting apply at boot, on most Linux distributions you
can create a file {HEADER}/etc/sysctl.d/20-qcp.conf{RESET} containing:
    {INFO}net.core.rmem_max={rmem}
    {INFO}net.core.wmem_max={wmem}{RESET}"
        );
    } else if cfg!(bsdish) {
        // Received wisdom about BSD kernels leads me to recommend 115% of the max. I'm not sure this is necessary.
        let size = std::cmp::max(rmem, wmem) * 115 / 100;
        let _ = write!(
            output,
            r"
🛠️ To set the kernel limits immediately, run the following command as root:
    {INFO}sysctl -w kern.ipc.maxsockbuf={size}{RESET}
To have this setting apply at boot, add this line to {HEADER}/etc/sysctl.conf{RESET}
or {HEADER}/etc/sysctl.d/udp_buffer.conf{RESET}:
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

We need the kernel UDP buffer size limit to be at least {rmem} for reading,
and {wmem} for writing.

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
    use super::super::test_buffers;
    use super::Platform;
    use crate::os::AbstractPlatform;

    #[test]
    fn config_paths() {
        assert!(
            Platform::system_ssh_config()
                .unwrap()
                .to_string_lossy()
                .contains("/etc/ssh/ssh_config")
        );
        let s = Platform::user_ssh_config().unwrap();
        assert!(s.to_string_lossy().contains("/home/"));
        let q = Platform::system_config_path().unwrap();
        assert!(q.to_string_lossy().starts_with("/etc/"));

        let pv = Platform::user_config_paths();
        assert!(pv.len() == 2);
        assert!(pv[0].to_string_lossy().contains("/home/"));
        assert!(pv[0].to_string_lossy().contains("/qcp/qcp.conf"));
        assert!(pv[1].to_string_lossy().contains("/home/"));
        assert!(pv[1].to_string_lossy().contains("/.qcp.conf"));
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
