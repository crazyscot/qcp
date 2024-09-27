// OS abstraction layer for qcp - Unix implementation
// (c) 2024 Ross Younger

use anyhow::Result;
use nix::sys::socket::{self, sockopt};
use std::net::UdpSocket;

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
}
