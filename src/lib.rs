//! QCP client & server library
// (c) 2024 Ross Younger

mod cli;
pub use cli::cli; // needs to be re-exported for the binary crate

pub mod cert;
pub mod client;
pub mod protocol;
pub mod server;
pub mod transport;
pub mod util;

mod console;

mod os;

/// Build-time info (from `built`)
pub mod build_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
