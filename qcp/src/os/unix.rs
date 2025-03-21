//! OS concretions for Unix platforms
// (c) 2024 Ross Younger

use crate::cli::styles::{ERROR, HEADER, INFO, RESET, SUCCESS, WARNING};
use crate::config::BASE_CONFIG_FILENAME;
use crate::util::socket::set_udp_buffer_sizes;

use super::SocketOptions;
use anyhow::Result;
use human_repr::HumanCount as _;
use nix::sys::socket::{self, sockopt};
use nix::unistd::{ROOT, geteuid};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::{net::UdpSocket, path::PathBuf};

impl SocketOptions for UdpSocket {
    fn get_sendbuf(&self) -> Result<usize> {
        #[cfg(target_os = "linux")]
        let divisor = 2;
        #[cfg(not(target_os = "linux"))]
        let divisor = 1;
        Ok(socket::getsockopt(self, sockopt::SndBuf)? / divisor)
    }

    fn set_sendbuf(&mut self, size: usize) -> Result<()> {
        socket::setsockopt(self, sockopt::SndBuf, &size)?;
        Ok(())
    }

    fn force_sendbuf(&mut self, size: usize) -> Result<()> {
        socket::setsockopt(self, sockopt::SndBufForce, &size)?;
        Ok(())
    }

    fn get_recvbuf(&self) -> Result<usize> {
        #[cfg(target_os = "linux")]
        let divisor = 2;
        #[cfg(not(target_os = "linux"))]
        let divisor = 1;
        Ok(socket::getsockopt(self, sockopt::RcvBuf)? / divisor)
    }

    fn set_recvbuf(&mut self, size: usize) -> Result<()> {
        socket::setsockopt(self, sockopt::RcvBuf, &size)?;
        Ok(())
    }

    fn force_recvbuf(&mut self, size: usize) -> Result<()> {
        socket::setsockopt(self, sockopt::RcvBufForce, &size)?;
        Ok(())
    }
}

/// Unix platform implementation
#[derive(Debug, Clone, Copy)]
pub struct Platform {}

impl super::AbstractPlatform for Platform {
    fn system_ssh_config() -> &'static str {
        "/etc/ssh/ssh_config"
    }

    fn user_ssh_config() -> Result<PathBuf> {
        let Some(mut pb) = dirs::home_dir() else {
            anyhow::bail!("could not determine home directory");
        };
        pb.push(".ssh");
        pb.push("config");
        Ok(pb)
    }

    fn user_config_dir() -> Option<PathBuf> {
        dirs::home_dir()
    }

    fn user_config_path() -> Option<PathBuf> {
        // ~/.<filename> for now
        let mut d: PathBuf = Self::user_config_dir()?;
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

    fn help_buffers_mode(rmem: u64, wmem: u64) {
        help_buffers_unix(rmem, wmem);
    }
}

fn test_buffers(wanted_recv: u64, wanted_send: u64) -> anyhow::Result<Option<String>> {
    let mut socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    set_udp_buffer_sizes(
        &mut socket,
        Some(wanted_send.try_into()?),
        Some(wanted_recv.try_into()?),
    )
}

fn help_buffers_unix(rmem: u64, wmem: u64) {
    println!(
        r"ℹ️  For best performance, it is necessary to set the kernel UDP buffer size limits.
This program attempts to automatically set buffer sizes for itself,
but this usually requires the kernel limits to be configured appropriately.

Testing this system..."
    );

    let result = test_buffers(rmem, wmem);
    let tested_ok = match result {
        Err(e) => {
            println!(
                r"
⚠️ {WARNING}Unable to test local UdpSocket parameters:{RESET} {e}
Outputting general advice..."
            );
            false
        }
        Ok(Some(s)) => {
            println!(
                r"
😞 {INFO}Test result:{RESET} {s}"
            );
            false
        }
        Ok(None) => true,
    };

    if tested_ok {
        println!(
            r"
🚀 {SUCCESS}Test result: Success{RESET}: {} (read) / {} (write).
Great!{RESET}",
            rmem.human_count_bytes(),
            wmem.human_count_bytes()
        );

        // root can usually override resource limits, so beware of false negatives
        if geteuid() != ROOT {
            println!(
                r"
💡 No administrative action is necessary. Happy qcp'ing!"
            );
            return;
        }

        println!(
            r"
⚠️  {WARNING}CAUTION: This process is running as user id 0 (root).{RESET}
This isn't a problem, but it means we can't tell whether ordinary users are
able to set suitable UDP buffer limits. If you like, rerun as a non-root user.
For now, outputting general advice."
        );
    }

    if cfg!(any(target_os = "linux",)) {
        println!(
            r"
🛠️  To set the kernel limits immediately, run the following command as root:
    {INFO}sysctl -w net.core.rmem_max={rmem} -w net.core.wmem_max={wmem}{RESET}

🛠️  To have this setting apply at boot, on most Linux distributions you
can create a file {HEADER}/etc/sysctl.d/20-qcp.conf{RESET} containing:
    {INFO}net.core.rmem_max={rmem}
    net.core.wmem_max={wmem}{RESET}"
        );
    } else if cfg!(any(
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "macos",
    )) {
        // Received wisdom about BSD kernels leads me to recommend 115% of the max. I'm not sure this is necessary.
        let size = std::cmp::max(rmem, wmem) * 115 / 100;
        println!(
            r"
🛠️ To set the kernel limits immediately, run the following command as root:
    {INFO}sysctl -w kern.ipc.maxsockbuf={size}{RESET}
To have this setting apply at boot, add this line to {HEADER}/etc/sysctl.conf{RESET}:
    {INFO}kern.ipc.maxsockbuf={size}{RESET}"
        );
    } else {
        println!(
            r"
{ERROR}Unknown unix build type!{RESET}

This build of qcp is configured for OS type '{}',
which is not covered by any of the current help messages.
Sorry about that! If you can fill in the details, please get in touch with
the developers.

We need the kernel UDP buffer size limit to be at least {rmem} for reading,
and {wmem} for writing.

There might be a sysctl or similar mechanism you can set to enable this.
Good luck!",
            std::env::consts::OS
        );
    }
}
