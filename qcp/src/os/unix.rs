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

pub struct Unix {}

impl Unix {
    pub fn preferred_udp_buffer_size() -> usize {
        1048576
    }

    pub fn get_sendbuf(socket: &UdpSocket) -> Result<usize> {
        Ok(socket::getsockopt(socket, sockopt::SndBuf)?)
    }

    pub fn set_sendbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::SndBuf, &size)?;
        Ok(())
    }

    pub fn force_sendbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::SndBufForce, &size)?;
        Ok(())
    }

    pub fn get_recvbuf(socket: &UdpSocket) -> Result<usize> {
        Ok(socket::getsockopt(socket, sockopt::RcvBuf)?)
    }

    pub fn set_recvbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::RcvBuf, &size)?;
        Ok(())
    }

    pub fn force_recvbuf(socket: &UdpSocket, size: usize) -> Result<()> {
        socket::setsockopt(socket, sockopt::RcvBufForce, &size)?;
        Ok(())
    }

    pub fn print_udp_buffer_size_help_message() {
        println!(
            r#"For best performance, it is necessary to set the kernel UDP buffer size limits.
This program attempts to automatically set buffer sizes for itself,
but this normally requires system administrator (root) privileges."#
        );

        if bsdish() {
            println!(
                r#"
To set the kernel limits immediately, run the following command as root:
    sysctl -w kern.ipc.maxsockbuf={size}
To have this setting apply at boot, add this line to /etc/sysctl.conf:
    kern.ipc.maxsockbuf={size}
            "#,
                size = Unix::preferred_udp_buffer_size() * 115 / 100
            );
        } else {
            println!(
                r#"
To set the kernel limits immediately, run the following command as root:
    sysctl -w net.core.rmem_max={bufsize} -w net.core.wmem_max={bufsize}

To have this setting apply at boot, on most Linux distributions you
can create a file /etc/sysctl.d/99-udp-buffer-for-qcp.conf containing:
    net.core.rmem_max={bufsize}
    net.core.wmem_max={bufsize}
"#,
                bufsize = Unix::preferred_udp_buffer_size() * 2
            );
        }
        // TODO add other OS-specific notes here
    }
}
