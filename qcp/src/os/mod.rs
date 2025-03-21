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
    /// On most platforms this will be `/etc/ssh/ssh_config`
    fn system_ssh_config() -> &'static str;

    /// Path to the user ssh config file.
    /// On most platforms this will be `${HOME}/.ssh/config`
    /// # Note
    /// This is a _theoretical_ path construction; it does not guarantee that the path actually exists.
    /// That is up to the caller to determine and reason about.
    /// # Errors
    /// If the current user's home directory could not be determined
    fn user_ssh_config() -> Result<PathBuf>;

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
