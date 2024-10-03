// OS abstraction layer for qcp - Unix implementation
// (c) 2024 Ross Younger

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

#[derive(Debug, Clone, Copy)]
/// OS abstraction layer for Unix-like platforms
pub struct Unix {}

impl Unix {
    // Caution: On Linux, according to socket(7), the kernel doubles the buffer size
    // you set in setsockopt, and returns the doubled value in getsockopt!

    /// Wrapper for getsockopt `SO_SNDBUF`
    pub fn get_sendbuf(socket: &UdpSocket) -> Result<usize> {
        Ok(socket::getsockopt(socket, sockopt::SndBuf)?)
    }

    /// Wrapper for setsockopt `SO_SNDBUF`
    pub fn set_sendbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::SndBuf, &size)?;
        Ok(())
    }

    /// Wrapper for setsockopt `SO_SNDBUFFORCE`
    pub fn force_sendbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::SndBufForce, &size)?;
        Ok(())
    }

    /// Wrapper for getsockopt `SO_RCVBUF`
    pub fn get_recvbuf(socket: &UdpSocket) -> Result<usize> {
        Ok(socket::getsockopt(socket, sockopt::RcvBuf)?)
    }

    /// Wrapper for setsockopt `SO_RCVBUF`
    pub fn set_recvbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::RcvBuf, &size)?;
        Ok(())
    }

    /// Wrapper for setsockopt `SO_RCVBUFFORCE`
    pub fn force_recvbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::RcvBufForce, &size)?;
        Ok(())
    }

    /// Outputs helpful information for the sysadmin
    pub fn print_udp_buffer_size_help_message(rmem: usize, wmem: usize) {
        println!(
            r#"For best performance, it is necessary to set the kernel UDP buffer size limits.
This program attempts to automatically set buffer sizes for itself,
but this normally requires system administrator (root) privileges."#
        );

        if bsdish() {
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
can create a file /etc/sysctl.d/99-udp-buffer-for-qcp.conf containing:
    net.core.rmem_max={rmem}
    net.core.wmem_max={wmem}
"#
            );
        }
        // TODO add other OS-specific notes here
    }
}
