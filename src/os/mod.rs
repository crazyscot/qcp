// OS abstraction layer for qcp
// (c) 2024 Ross Younger

#[cfg(unix)]
mod unix;

#[cfg(unix)]
pub(crate) use crate::os::unix::Unix as os;

static_assertions::assert_cfg!(unix, "This OS is not yet supported");
