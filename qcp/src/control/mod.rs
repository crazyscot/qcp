//! Control protocol implementation
// (c) 2025 Ross Younger

mod channel;
mod endpoint;
mod ssh_process;

pub(crate) use channel::{ControlChannel, ControlChannelServerInterface, stdio_channel};
pub(crate) use endpoint::create_endpoint;
pub(crate) use ssh_process::create;

#[cfg(test)]
pub(crate) use channel::{MockControlChannelServerInterface, ServerResult};
#[cfg(all(test, unix))]
pub(crate) use ssh_process::create_fake;
