//! OS abstraction layer
// (c) 2024 Ross Younger

use std::{net::UdpSocket, path::PathBuf};

use anyhow::Result;
use cfg_if::cfg_if;
use rustix::net::sockopt as RustixSO;

#[cfg(unix)]
use rustix::fd::AsFd as SocketType;

/// Platform-specific: Modify values returned from getsockopt(SndBuf | RcvBuf).
const fn buffer_size_fix(s: usize) -> usize {
    if cfg!(linux) { s / 2 } else { s }
}
/// OS abstraction trait providing access to socket options.
pub trait SocketOptions: SocketType {
    /// Wrapper for `getsockopt SO_SNDBUF`.
    ///
    /// This function returns the actual socket buffer size, which works around
    /// platform discrepancies.
    /// (For example: On Linux, the internal buffer allocation is _double_ the size
    /// set with setsockopt, yet getsockopt returns the doubled value.)
    fn get_sendbuf(&self) -> Result<usize> {
        Ok(buffer_size_fix(RustixSO::socket_send_buffer_size(self)?))
    }
    /// Wrapper for setsockopt `SO_SNDBUF`
    fn set_sendbuf(&mut self, size: usize) -> Result<()> {
        RustixSO::set_socket_send_buffer_size(self, size)?;
        Ok(())
    }
    /// Wrapper for setsockopt `SO_SNDBUFFORCE` (where available; quietly returns Ok if not supported by platform)
    #[allow(clippy::used_underscore_binding)]
    fn force_sendbuf(&mut self, _size: usize) -> Result<()> {
        cfg_if! {
            if #[cfg(linux)] {
                RustixSO::set_socket_send_buffer_size_force(self, _size)?;
            }
        }
        Ok(())
    }

    /// Wrapper for `getsockopt SO_RCVBUF`.
    ///
    /// This function returns the actual socket buffer size, which works around
    /// platform discrepancies.
    /// (For example: On Linux, the internal buffer allocation is _double_ the size
    /// set with setsockopt, yet getsockopt returns the doubled value.)
    fn get_recvbuf(&self) -> Result<usize> {
        Ok(buffer_size_fix(RustixSO::socket_recv_buffer_size(self)?))
    }

    /// Wrapper for setsockopt `SO_RCVBUF`
    fn set_recvbuf(&mut self, size: usize) -> Result<()> {
        RustixSO::set_socket_recv_buffer_size(self, size)?;
        Ok(())
    }
    /// Wrapper for setsockopt `SO_RCVBUFFORCE` (where available; quietly returns Ok if not supported by platform)
    #[allow(clippy::used_underscore_binding)]
    fn force_recvbuf(&mut self, _size: usize) -> Result<()> {
        cfg_if! {
            if #[cfg(linux)] {
                RustixSO::set_socket_recv_buffer_size_force(self, _size)?;
            }
        }
        Ok(())
    }

    /// Indicates whether `SO_SNDBUFFORCE` and `SO_RCVBUFFORCE` are available on this platform
    fn has_force_sendrecvbuf(&self) -> bool {
        cfg!(linux)
    }
}

impl SocketOptions for UdpSocket {}

/// General platform abstraction trait
///
/// The active implementation (as configured by cargo) is re-exported by this crate as `Platform`.
///
/// Usage:
/// ```text
///    use qcp::os::Platform;
///    use qcp::os::AbstractPlatform as _;
///    println!("{}", Platform::system_ssh_config());
/// ```
pub trait AbstractPlatform {
    /// Path to the system ssh config file.
    /// On most platforms this will be `/etc/ssh/ssh_config`.
    /// Returns None if the path could not be determined.
    fn system_ssh_config() -> Option<PathBuf>;

    /// Path to the user ssh config file.
    /// On most platforms this will be `${HOME}/.ssh/config`.
    /// Returns None if the current user's home directory could not be determined
    /// # Note
    /// This is a _theoretical_ path construction; it does not guarantee that the path actually exists.
    /// That is up to the caller to determine and reason about.
    fn user_ssh_config() -> Option<PathBuf>;

    /// The absolute path to the user configuration file, if one is defined on this platform.
    ///
    /// If somehow we could not determine the path to use, returns None (and may emit a warning).
    fn user_config_path() -> Option<PathBuf>;

    /// The absolute path to the system configuration file, if one is defined on this platform.
    fn system_config_path() -> Option<PathBuf>;

    /// Implementation of `--help-buffers` mode.
    ///
    /// This is a help mode for the sysadmin.
    /// It outputs useful information and advice.
    /// It may, if applicable, check the OS configuration to improve the quality of the advice
    /// given.
    fn help_buffers_mode(rmem: u64, wmem: u64);
}

mod unix;

pub use unix::Platform as UnixPlatform;

#[cfg(unix)]
pub(crate) use UnixPlatform as Platform;

static_assertions::assert_cfg!(unix, "This OS is not yet supported");
