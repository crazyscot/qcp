// OS abstraction layer for qcp
// (c) 2024 Ross Younger

use anyhow::Result;

/// OS abstraction trait providing access to socket options
pub(crate) trait SocketOptions {
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

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub(crate) use unix::*;

static_assertions::assert_cfg!(unix, "This OS is not yet supported");
