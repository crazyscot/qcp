// qcp socket wrangling
// (c) 2024 Ross Younger

use crate::os::os;
use human_repr::HumanCount as _;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
use tracing::{debug, info, warn};

/// Set the buffer size options on a UDP socket.
/// Returns an optional warning message.
pub fn set_udp_buffer_sizes(socket: &UdpSocket) -> anyhow::Result<Option<String>> {
    let wanted = os::preferred_udp_buffer_size();
    let mut send = os::get_sendbuf(socket)?;
    let mut recv = os::get_recvbuf(socket)?;
    debug!(
        "system default socket buffer sizes are {} send, {} receive",
        send.human_count_bare(),
        recv.human_count_bare()
    );
    if send < wanted {
        let _ = os::set_sendbuf(socket, wanted);
        send = os::get_sendbuf(socket)?;
    }
    if recv < wanted {
        let _ = os::set_recvbuf(socket, wanted);
        recv = os::get_recvbuf(socket)?;
    }

    let mut force_err: Option<anyhow::Error> = None;
    if send < wanted {
        force_err = os::force_sendbuf(socket, wanted).err();
    }
    if recv < wanted {
        force_err = os::force_recvbuf(socket, wanted).err().or(force_err);
    }

    send = os::get_sendbuf(socket)?;
    recv = os::get_recvbuf(socket)?;
    let mut message: Option<String> = None;
    if send < wanted || recv < wanted {
        message = Some(
        format!(
            "Unable to set UDP send buffer sizes (got send {}, receive {}; wanted {}). This may affect performance.",
            send.human_count_bytes(),
            recv.human_count_bytes(),
            wanted.human_count_bytes()
        ));
        if let Some(e) = force_err {
            warn!("While attempting to set kernel buffer size, this happened: {e}")
        }
        let mut args = std::env::args();
        let ego = args.next().unwrap_or("<this program>".to_string());
        info!("For more information, run: `{} --help-socket-bufsize`", ego);
        // TODO: Make buffer size configurable, the user might have a better idea than we do of what's good for their network.
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
        super::set_udp_buffer_sizes(&sock)?;
        Ok(())
    }
}
