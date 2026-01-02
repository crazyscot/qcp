//! OS abstraction layer and OS-specific documentation
//!
//! **NOTE:** The appropriate Platform for the build is re-exported as `Platform` at compile time.
//!
// (c) 2024 Ross Younger

use std::{
    net::{Ipv4Addr, SocketAddrV4, UdpSocket},
    path::PathBuf,
    sync::Once,
};

use crate::util::socket::UdpBufferSizeData;
use anyhow::Result;
use cfg_if::cfg_if;
use rustix::net::sockopt as RustixSO;

pub mod osx;
#[cfg(unix)]
pub mod unix; // doesn't compile on windows
pub mod windows;

static_assertions::assert_cfg!(any(unix, windows), "This OS is not currently supported");

const TESTING_BUFFERS_MESSAGE: &str = r"ℹ️  For best performance, it is necessary to set the kernel UDP buffer size limits.
This program attempts to automatically set buffer sizes for itself,
but this usually requires the kernel limits to be configured appropriately.

Testing this system...";

cfg_if! {
    if #[cfg(unix)] {
        use rustix::fd::AsFd as SocketType;
        /// For docs
        pub use unix::UnixPlatform;
        /// For real
        pub use UnixPlatform as Platform;
    }
}

cfg_if! {
    if #[cfg(windows)] {
        use rustix::fd::AsSocket as SocketType;
        pub use WindowsPlatform as Platform;
    }
}

// Include the windows layer in unix dev builds, so it's included in coverage runs.
#[cfg(windows_or_dev)]
pub use windows::WindowsPlatform;

/// Platform-specific: Modify values returned from getsockopt(SndBuf | RcvBuf).
const fn buffer_size_fix(s: usize) -> usize {
    if cfg!(linux) { s / 2 } else { s }
}

/// Platform initialisation hook.
/// This must be called at least once before using platform features.
/// # Panics
/// On Windows, if `wsa_startup()` fails.
pub fn initialise_platform() -> Result<()> {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        cfg_if! {
            if #[cfg(windows)] {
                let _ = rustix::net::wsa_startup().unwrap();
                // We don't provide a hook calling wsa_cleanup; there's no point as process is exiting anyway.
            }
        }
    });
    Ok(())
}

mod private {
    pub trait SealedSocket: super::SocketType {}
    impl SealedSocket for super::UdpSocket {}
}

/// OS abstraction trait providing ergonomic access to additional Berkeley socket options.
///
/// **This is a sealed trait** : it only works for `UdpSocket`.
pub trait SocketOptions: private::SealedSocket
where
    Self: Sized,
{
    // Sized is needed by the Windows AsSocket polyfill

    /// Wrapper for `getsockopt SO_SNDBUF`.
    ///
    /// This function returns the actual socket buffer size, which works around
    /// platform discrepancies.
    /// (For example: On Linux, the internal buffer allocation is _double_ the size
    /// set with setsockopt, yet getsockopt returns the doubled value.)
    fn get_sendbuf(&self) -> Result<usize> {
        initialise_platform()?;
        Ok(buffer_size_fix(RustixSO::socket_send_buffer_size(self)?))
    }
    /// Wrapper for setsockopt `SO_SNDBUF`
    fn set_sendbuf(&mut self, size: usize) -> Result<()> {
        initialise_platform()?;
        RustixSO::set_socket_send_buffer_size(self, size)?;
        Ok(())
    }
    /// Wrapper for setsockopt `SO_SNDBUFFORCE` (where available; quietly returns Ok if not supported by platform)
    #[allow(clippy::used_underscore_binding)]
    fn force_sendbuf(&mut self, _size: usize) -> Result<()> {
        initialise_platform()?;
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
        initialise_platform()?;
        Ok(buffer_size_fix(RustixSO::socket_recv_buffer_size(self)?))
    }

    /// Wrapper for setsockopt `SO_RCVBUF`
    fn set_recvbuf(&mut self, size: usize) -> Result<()> {
        initialise_platform()?;
        RustixSO::set_socket_recv_buffer_size(self, size)?;
        Ok(())
    }
    /// Wrapper for setsockopt `SO_RCVBUFFORCE` (where available; quietly returns Ok if not supported by platform)
    #[allow(clippy::used_underscore_binding)]
    fn force_recvbuf(&mut self, _size: usize) -> Result<()> {
        initialise_platform()?;
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

/// This is the only place UdpSocket is defined.
impl SocketOptions for UdpSocket {}

/// Can we set the kernel buffer sizes we want to?
fn test_udp_buffers(wanted_recv: u64, wanted_send: u64) -> anyhow::Result<UdpBufferSizeData> {
    let mut socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    crate::util::socket::set_udp_buffer_sizes(
        &mut socket,
        Some(wanted_send.try_into()?),
        Some(wanted_recv.try_into()?),
    )
}

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

    /// The absolute path to an extra user configuration file, if one is defined on this platform.
    fn user_config_path_extra() -> Option<PathBuf>;

    /// The list of absolute paths to possible user configuration files.
    /// This always includes `dirs::config_dir()/qcp/qcp.conf`, and may include others
    /// depending on the platform.
    /// The order of the paths is significant; settings are applied in the order they are found.
    #[must_use]
    fn user_config_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(mut pb) = dirs::config_dir() {
            pb.push("qcp");
            pb.push(crate::config::BASE_CONFIG_FILENAME);
            paths.push(pb);
        }
        if let Some(path) = Self::user_config_path_extra() {
            paths.push(path);
        }
        paths
    }

    /// The absolute path to the system configuration file, if one is defined on this platform.
    fn system_config_path() -> Option<PathBuf>;

    /// Location of the system ssh config file, if one is defined on this platform.
    fn system_ssh_dir_path() -> Option<PathBuf>;

    /// Implementation of `--help-buffers` mode.
    ///
    /// This is a help mode for the sysadmin.
    /// It returns a string, intended for the console, with useful information and advice.
    /// It may, if applicable, check the OS configuration to improve the quality of the advice
    /// given.
    fn help_buffers_mode(udp: u64) -> String;

    /// OS-specific additions to the logic that detects a string as a local path.
    /// (I'm looking at you, Windows. Case in point: `qcp host:file c:/users/me/file` !)
    #[must_use]
    fn override_path_is_local(_path: &str) -> bool {
        false
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::test_udp_buffers;

    #[test]
    fn test_buffers_small_ok() {
        assert!(
            test_udp_buffers(131_072, 131_072)
                .unwrap()
                .warning
                .is_none()
        );
    }
    #[test]
    fn test_buffers_gigantic_err() {
        let big = 2u64.pow(60);
        let result = test_udp_buffers(big, big).unwrap();
        eprintln!("{result:?}");
        assert!(result.warning.is_some());
    }
}
