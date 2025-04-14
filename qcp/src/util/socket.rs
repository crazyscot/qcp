//! Socket wrangling
// (c) 2024 Ross Younger

use crate::{os::SocketOptions as _, protocol::control::ConnectionType};
use human_repr::HumanCount as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use tracing::{debug, info, warn};

use super::PortRange;

#[derive(Debug, Clone)]
pub(crate) struct UdpBufferSizeData {
    pub(crate) ok: bool,
    pub(crate) send: usize,
    pub(crate) recv: usize,
    pub(crate) warning: Option<String>,
}

/// Set the buffer size options on a UDP socket.
/// May return a warning message, if we weren't able to do so.
pub(crate) fn set_udp_buffer_sizes(
    socket: &mut UdpSocket,
    wanted_send: Option<usize>,
    wanted_recv: Option<usize>,
) -> anyhow::Result<UdpBufferSizeData> {
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
    let ok = if send < wanted_send || recv < wanted_recv {
        let msg = format!(
            "Unable to set UDP buffer sizes (send wanted {}, got {}; receive wanted {}, got {}). This may affect performance.",
            wanted_send.human_count_bytes(),
            send.human_count_bytes(),
            wanted_recv.human_count_bytes(),
            recv.human_count_bytes(),
        );
        message = Some(msg);
        if let Some(e) = force_err {
            warn!("While attempting to set kernel buffer size, this happened: {e:?}");
        }
        info!(
            "For more information, run: `{ego} --help-buffers`",
            ego = std::env::args()
                .next()
                .unwrap_or("<this program>".to_string()),
        );
        false
        // SOMEDAY: We might offer to set sysctl, write sysctl files, etc. if run as root.
    } else {
        debug!(
            "UDP buffer sizes set to {} send, {} receive",
            send.human_count_bare(),
            recv.human_count_bare()
        );
        true
    };
    Ok(UdpBufferSizeData {
        ok,
        send,
        recv,
        warning: message,
    })
}

/// Creates and binds a UDP socket from a restricted range of local ports, for a given local address
pub(crate) fn bind_range_for_address(
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
pub(crate) fn bind_range_for_family(
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
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use crate::{os::SocketOptions as _, util::tracing::setup_tracing_for_tests};
    use rusty_fork::rusty_fork_test;
    use std::net::UdpSocket;

    // To see how this behaves with privileges, you might:
    //    sudo -E cargo test -- util::socket::test::set_socket_bufsize
    // The program executable name reported by info!() will not be very useful, but you could probably have guessed that :-)
    rusty_fork_test! {
        #[test]
        fn set_udp_buffer_sizes() {
            setup_tracing_for_tests(); // this modifies global state, so needs to be run in a fork
            let mut sock = UdpSocket::bind("0.0.0.0:0").unwrap();
            let _ = super::set_udp_buffer_sizes(&mut sock, Some(4_194_304), Some(10_485_760)).unwrap();
        }

        #[test]
        fn set_socket_bufsize_direct() {
            let mut sock = UdpSocket::bind("0.0.0.0:0").unwrap();
            cfg_if::cfg_if! {
                if #[cfg(linux)] {
                    assert!(sock.has_force_sendrecvbuf());
                    let _ = sock.force_sendbuf(128).unwrap_err();
                    let _ = sock.force_recvbuf(128).unwrap_err();
                }
            }
        }
    }
}
