//! Socket wrangling
// (c) 2024 Ross Younger

use crate::{os::SocketOptions as _, protocol::control::ConnectionType};
use human_repr::HumanCount as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
use tracing::{debug, info, warn};

use super::PortRange;

/// Set the buffer size options on a UDP socket.
/// May return a warning message, if we weren't able to do so.
pub fn set_udp_buffer_sizes(
    socket: &mut UdpSocket,
    wanted_send: Option<usize>,
    wanted_recv: Option<usize>,
) -> anyhow::Result<Option<String>> {
    let mut send = socket.get_sendbuf()?;
    let mut recv = socket.get_recvbuf()?;
    debug!(
        "system default socket buffer sizes are {} send, {} receive",
        send.human_count_bare(),
        recv.human_count_bare()
    );
    let mut force_err: Option<anyhow::Error> = None;
    let wanted_send = wanted_send.unwrap_or(send);
    let wanted_recv = wanted_recv.unwrap_or(recv);

    if send < wanted_send {
        let _ = socket.set_sendbuf(wanted_send);
        send = socket.get_sendbuf()?;
    }
    if send < wanted_send {
        force_err = socket.force_sendbuf(wanted_send).err();
    }
    if recv < wanted_recv {
        let _ = socket.set_recvbuf(wanted_recv);
        recv = socket.get_recvbuf()?;
    }
    if recv < wanted_recv {
        force_err = socket.force_recvbuf(wanted_recv).err().or(force_err);
    }

    send = socket.get_sendbuf()?;
    recv = socket.get_recvbuf()?;
    let mut message: Option<String> = None;
    if send < wanted_send || recv < wanted_recv {
        let msg = format!(
            "Unable to set UDP buffer sizes (send wanted {}, got {}; receive wanted {}, got {}). This may affect performance.",
            wanted_send.human_count_bytes(),
            send.human_count_bytes(),
            wanted_recv.human_count_bytes(),
            recv.human_count_bytes(),
        );
        warn!("{msg}");
        message = Some(msg);
        if let Some(e) = force_err {
            warn!("While attempting to set kernel buffer size, this happened: {e}");
        }
        info!(
            "For more information, run: `{ego} --help-buffers`",
            ego = std::env::args()
                .next()
                .unwrap_or("<this program>".to_string()),
        );
        // SOMEDAY: We might offer to set sysctl, write sysctl files, etc. if run as root.
    } else {
        debug!(
            "UDP buffer sizes set to {} send, {} receive",
            send.human_count_bare(),
            recv.human_count_bare()
        );
    }
    Ok(message)
}

/// Creates and binds a UDP socket for the address family necessary to reach the given peer address
pub fn bind_unspecified_for(peer: &SocketAddr) -> anyhow::Result<std::net::UdpSocket> {
    let addr: SocketAddr = match peer {
        SocketAddr::V4(_) => SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0).into(),
        SocketAddr::V6(_) => SocketAddrV6::new(Ipv6Addr::UNSPECIFIED, 0, 0, 0).into(),
    };
    Ok(UdpSocket::bind(addr)?)
}

/// Creates and binds a UDP socket from a restricted range of local ports, using the address family necessary to reach the given peer address
pub fn bind_range_for_peer(
    peer: &SocketAddr,
    range: PortRange,
) -> anyhow::Result<std::net::UdpSocket> {
    let addr: IpAddr = match peer {
        SocketAddr::V4(_) => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        SocketAddr::V6(_) => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    bind_range_for_address(addr, range)
}

/// Creates and binds a UDP socket from a restricted range of local ports, for a given local address
pub fn bind_range_for_address(
    addr: IpAddr,
    range: PortRange,
) -> anyhow::Result<std::net::UdpSocket> {
    if range.begin == range.end {
        return Ok(UdpSocket::bind(SocketAddr::new(addr, range.begin))?);
    }
    for port in range.begin..range.end {
        let result = UdpSocket::bind(SocketAddr::new(addr, port));
        if let Ok(sock) = result {
            debug!("bound endpoint to UDP port {port}");
            return Ok(sock);
        }
    }
    anyhow::bail!("failed to bind a port in the given range");
}

/// Creates and binds a UDP socket from a restricted range of local ports, for the unspecified address of the given address family
pub fn bind_range_for_family(
    family: ConnectionType,
    range: PortRange,
) -> anyhow::Result<std::net::UdpSocket> {
    let addr = match family {
        ConnectionType::Ipv4 => IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        ConnectionType::Ipv6 => IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    };
    bind_range_for_address(addr, range)
}

#[cfg(test)]
mod test {
    use crate::util::tracing::setup_tracing_for_tests;
    use std::net::UdpSocket;

    // To see how this behaves with privileges, you might:
    //    sudo -E cargo test -- util::socket::test::set_socket_bufsize
    // The program executable name reported by info!() will not be very useful, but you could probably have guessed that :-)
    #[test]
    fn set_socket_bufsize() -> anyhow::Result<()> {
        setup_tracing_for_tests();
        let mut sock = UdpSocket::bind("0.0.0.0:0")?;
        let _ = super::set_udp_buffer_sizes(&mut sock, Some(1_048_576), Some(10_485_760))?;
        Ok(())
    }
}
