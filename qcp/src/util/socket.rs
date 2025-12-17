//! Socket wrangling
// (c) 2024 Ross Younger

use crate::{os::SocketOptions as _, protocol::control::ConnectionType};
use human_repr::HumanCount as _;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, UdpSocket};
use tracing::{debug, info, warn};

use super::PortRange;

#[derive(Debug, Clone)]
pub(crate) struct UdpBufferSizeData {
    #[allow(dead_code)] // `ok` is not used on windows
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
    use crate::{protocol::control::ConnectionType, util::PortRange};
    use rusty_fork::rusty_fork_test;
    use std::net::{IpAddr, Ipv4Addr, UdpSocket};

    use super::{bind_range_for_address, bind_range_for_family};

    const UNSPECIFIED: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

    // set_udp_buffer_sizes is tested by the OS-specific tests

    // To see how this behaves with privileges, you might:
    //    sudo -E cargo test -- util::socket::test::set_socket_bufsize
    // The program executable name reported by info!() will not be very useful, but you could probably have guessed that :-)
    rusty_fork_test! {
        #[test]
        #[allow(clippy::items_after_statements)]
        fn set_socket_bufsize_direct() {
            if cfg!(linux) {
                use crate::os::SocketOptions as _;
                let mut sock = UdpSocket::bind("0.0.0.0:0").unwrap();
                assert!(sock.has_force_sendrecvbuf());
                let _ = sock.force_sendbuf(128).unwrap_err();
                let _ = sock.force_recvbuf(128).unwrap_err();
            } else {
                let _ = UdpSocket::bind("0.0.0.0:0").unwrap();
            }
        }
    }

    #[test]
    fn bind_range() {
        let range = PortRange {
            begin: 1,
            end: 65535,
        };
        let _s = bind_range_for_address(UNSPECIFIED, range).unwrap();
    }

    #[cfg_attr(target_os = "macos", ignore)] // GitHub OSX runners allow binding to ports <1024...
    #[cfg_attr(msvc, ignore)] // MSVC doesn't implement the unix privilege check, obviously
    #[test]
    fn bind_range_fails_non_root() {
        let range = PortRange { begin: 1, end: 2 };
        let r = bind_range_for_address(UNSPECIFIED, range);
        eprintln!("{r:?}");
        let _ = r.unwrap_err();
    }

    #[test]
    fn bind_ipv6() {
        let range = PortRange::default();
        let s = match bind_range_for_family(ConnectionType::Ipv6, range) {
            Ok(s) => s,
            Err(err) => {
                let is_ipv6_unsupported = err.chain().any(|cause| {
                    let Some(io_err) = cause.downcast_ref::<std::io::Error>() else {
                        return false;
                    };
                    matches!(io_err.raw_os_error(), Some(97 | 47 | 10047))
                });
                if is_ipv6_unsupported {
                    eprintln!("IPv6 not supported on this host; skipping: {err:#}");
                    return;
                }
                panic!("{err:#}");
            }
        };
        let a = s.local_addr().unwrap();
        assert!(a.is_ipv6());
    }
}
