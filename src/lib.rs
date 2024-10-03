//! QCP client & server library
// (c) 2024 Ross Younger

/// X509 certificate helpers
pub mod cert;
mod cli;
pub use cli::cli;
/// qcp client
pub mod client;
mod console;
/// OS abstraction layer
pub mod os;
/// qcp's protocol structures
pub mod protocol;
/// qcp server
pub mod server;
mod styles;
/// QUIC transport configuration
pub mod transport;
/// Utilities
pub mod util;

/// Build-time info (from `built`)
pub mod build_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
