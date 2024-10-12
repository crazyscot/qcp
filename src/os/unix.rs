// OS abstraction layer for qcp - Unix implementation
// (c) 2024 Ross Younger

use super::SocketOptions;
use anyhow::Result;
use nix::sys::socket::{self, sockopt};
use std::net::UdpSocket;

fn bsdish() -> bool {
    cfg!(any(
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "freebsd",
        target_os = "macos"
    ))
}

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

#[derive(Debug, Clone, Copy)]
/// OS abstraction layer for Unix-like platforms
pub(crate) struct Unix {}

impl Unix {
    /// Outputs helpful information for the sysadmin
    pub(crate) fn print_udp_buffer_size_help_message(rmem: u64, wmem: u64) {
        println!(
            r#"For best performance, it is necessary to set the kernel UDP buffer size limits.
This program attempts to automatically set buffer sizes for itself,
but this requires elevated privileges."#
        );

        if bsdish() {
            // Received wisdom about BSD kernels leads me to recommend 115% of the max. I'm not sure this is necessary.
            let size = std::cmp::max(rmem, wmem) * 115 / 100;
            println!(
                r#"
To set the kernel limits immediately, run the following command as root:
    sysctl -w kern.ipc.maxsockbuf={size}
To have this setting apply at boot, add this line to /etc/sysctl.conf:
    kern.ipc.maxsockbuf={size}
            "#
            );
        } else {
            println!(
                r#"
To set the kernel limits immediately, run the following command as root:
    sysctl -w net.core.rmem_max={rmem} -w net.core.wmem_max={wmem}

To have this setting apply at boot, on most Linux distributions you
can create a file /etc/sysctl.d/60-qcp.conf containing:
    net.core.rmem_max={rmem}
    net.core.wmem_max={wmem}
"#
            );
        }
        // TODO add other OS-specific notes here
    }
}
