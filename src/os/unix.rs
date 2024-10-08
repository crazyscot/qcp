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
pub(crate) struct Unix {}

impl Unix {
    /// Wrapper for getsockopt `SO_SNDBUF`.
    /// On Linux, this call halves the number returned from the kernel.
    /// This takes account of kernel behaviour: the internal buffer
    /// allocation is _double_ the size you set with setsockopt,
    /// and getsockopt returns the doubled value.
    pub(crate) fn get_sendbuf(socket: &UdpSocket) -> Result<usize> {
        #[cfg(target_os = "linux")]
        let divisor = 2;
        #[cfg(not(target_os = "linux"))]
        let divisor = 1;
        Ok(socket::getsockopt(socket, sockopt::SndBuf)? / divisor)
    }

    /// Wrapper for setsockopt `SO_SNDBUF`
    pub(crate) fn set_sendbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::SndBuf, &size)?;
        Ok(())
    }

    /// Wrapper for setsockopt `SO_SNDBUFFORCE`
    pub(crate) fn force_sendbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::SndBufForce, &size)?;
        Ok(())
    }

    /// Wrapper for getsockopt `SO_RCVBUF`.
    /// On Linux, this call halves the number returned from the kernel.
    /// This takes account of kernel behaviour: the internal buffer
    /// allocation is _double_ the size you set with setsockopt,
    /// and getsockopt returns the doubled value.
    pub(crate) fn get_recvbuf(socket: &UdpSocket) -> Result<usize> {
        #[cfg(target_os = "linux")]
        let divisor = 2;
        #[cfg(not(target_os = "linux"))]
        let divisor = 1;
        Ok(socket::getsockopt(socket, sockopt::RcvBuf)? / divisor)
    }

    /// Wrapper for setsockopt `SO_RCVBUF`
    pub(crate) fn set_recvbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::RcvBuf, &size)?;
        Ok(())
    }

    /// Wrapper for setsockopt `SO_RCVBUFFORCE`
    pub(crate) fn force_recvbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::RcvBufForce, &size)?;
        Ok(())
    }

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
can create a file /etc/sysctl.d/99-udp-buffer-for-qcp.conf containing:
    net.core.rmem_max={rmem}
    net.core.wmem_max={wmem}
"#
            );
        }
        // TODO add other OS-specific notes here
    }
}
