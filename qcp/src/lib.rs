// QCP client & server library
// (c) 2024 Ross Younger

pub mod cert;
pub mod client;
pub mod protocol;
pub mod server;
pub mod styles;
pub mod util;

/// Build-time info (from `built`)
pub mod build_info {
    include!(concat!(env!("OUT_DIR"), "/built.rs"));
}
