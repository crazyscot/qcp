//! Control protocol implementation
// (c) 2025 Ross Younger

mod channel;
mod endpoint;
mod process;

pub(crate) use channel::{ControlChannel, ControlChannelServerInterface, stdio_channel};
pub(crate) use endpoint::create_endpoint;
pub(crate) use process::Ssh as ClientSsh;

#[cfg(test)]
pub(crate) use channel::{MockControlChannelServerInterface, ServerResult};
