// qcp socket wrangling
// (c) 2024 Ross Younger

use crate::os::os;
use human_repr::HumanCount as _;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
use tracing::{debug, info, warn};

/// Set the buffer size options on a UDP socket.
/// May return a warning message, if we weren't able to do so.
pub fn set_udp_buffer_sizes(
    socket: &UdpSocket,
    wanted_send: usize,
    wanted_recv: usize,
    bandwidth_limit: u64,
    rtt_ms: u16,
) -> anyhow::Result<Option<String>> {
    let mut send = os::get_sendbuf(socket)?;
    let mut recv = os::get_recvbuf(socket)?;
    debug!(
        "system default socket buffer sizes are {} send, {} receive",
        send.human_count_bare(),
        recv.human_count_bare()
    );
    if send < wanted_send {
        let _ = os::set_sendbuf(socket, wanted_send);
        send = os::get_sendbuf(socket)?;
    }
    if recv < wanted_recv {
        let _ = os::set_recvbuf(socket, wanted_recv);
        recv = os::get_recvbuf(socket)?;
    }

    let mut force_err: Option<anyhow::Error> = None;
    if send < wanted_send {
        force_err = os::force_sendbuf(socket, wanted_send).err();
    }
    if recv < wanted_recv {
        force_err = os::force_recvbuf(socket, wanted_recv).err().or(force_err);
    }

    send = os::get_sendbuf(socket)?;
    recv = os::get_recvbuf(socket)?;
    let mut message: Option<String> = None;
    if send < wanted_send || recv < wanted_recv {
        message = Some(format!(
            "Unable to set UDP buffer sizes (send wanted {}, got {}; receive wanted {}, got {}). This may affect performance.",
            wanted_send.human_count_bytes(),
            send.human_count_bytes(),
            wanted_recv.human_count_bytes(),
            recv.human_count_bytes(),
        ));
        if let Some(e) = force_err {
            warn!("While attempting to set kernel buffer size, this happened: {e}");
        }
        let mut args = std::env::args();
        let ego = args.next().unwrap_or("<this program>".to_string());
        info!(
            "For more information, run: `{} --help-buffers --bandwidth {bandwidth_limit} --rtt {rtt_ms}`",
            ego
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
        let sock = UdpSocket::bind("0.0.0.0:0")?;
        let _ = super::set_udp_buffer_sizes(&sock, 1_048_576, 10_485_760, 12_000_000, 300)?;
        Ok(())
    }
}
