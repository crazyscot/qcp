//! OS abstraction layer
// (c) 2024 Ross Younger

use std::path::PathBuf;

use anyhow::Result;

/// OS abstraction trait providing access to socket options
pub trait SocketOptions {
    /// Wrapper for getsockopt `SO_SNDBUF`.
    /// On Linux, this call halves the number returned from the kernel.
    /// This takes account of kernel behaviour: the internal buffer
    /// allocation is _double_ the size you set with setsockopt,
    /// and getsockopt returns the doubled value.
    fn get_sendbuf(&self) -> Result<usize>;
    /// Wrapper for setsockopt `SO_SNDBUF`
    fn set_sendbuf(&mut self, size: usize) -> Result<()>;
    /// Wrapper for setsockopt `SO_SNDBUFFORCE` (where available; will error if not supported on system)
    fn force_sendbuf(&mut self, size: usize) -> Result<()>;

    /// Wrapper for getsockopt `SO_RCVBUF`.
    /// On Linux, this call halves the number returned from the kernel.
    /// This takes account of kernel behaviour: the internal buffer
    /// allocation is _double_ the size you set with setsockopt,
    /// and getsockopt returns the doubled value.
    fn get_recvbuf(&self) -> Result<usize>;
    /// Wrapper for setsockopt `SO_RCVBUF`
    fn set_recvbuf(&mut self, size: usize) -> Result<()>;
    /// Wrapper for setsockopt `SO_RCVBUFFORCE` (where available; will error if not supported on system)
    fn force_recvbuf(&mut self, size: usize) -> Result<()>;
}

/// General platform abstraction trait.
/// The active implementation should be pulled into this crate
/// Implementations should be called `Platform`, e.g. [unix::Platform].
///
/// Usage:
/// ```
///    use qcp::os::Platform;
///    use qcp::os::AbstractPlatform as _;
///    println!("{}", Platform::system_ssh_config());
/// ```
pub trait AbstractPlatform {
    /// Path to the system ssh config file.
    /// On most platforms this will be `/etc/ssh/ssh_config`
    fn system_ssh_config() -> &'static str;

    /// The directory to store user configuration files in.
    ///
    /// On Unix platforms this is the traditional home directory.
    ///
    /// If somehow we could not determine the directory to use, returns None (and may emit a warning).
    fn user_config_dir() -> Option<PathBuf>;

    /// The absolute path to the user configuration file, if one is defined on this platform.
    ///
    /// If somehow we could not determine the path to use, returns None (and may emit a warning).
    fn user_config_path() -> Option<PathBuf>;

    /// The absolute path to the system configuration file, if one is defined on this platform.
    fn system_config_path() -> Option<PathBuf>;
}

#[cfg(any(unix, doc))]
mod unix;

#[cfg(any(unix, doc))]
pub use unix::*;

static_assertions::assert_cfg!(unix, "This OS is not yet supported");
