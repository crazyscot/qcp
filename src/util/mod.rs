//! General utility code that didn't fit anywhere else
// (c) 2024 Ross Younger

mod dns;
pub use dns::{lookup_host_by_family, AddressFamily};

pub mod cert;
pub mod io;
pub mod socket;
pub mod stats;
pub mod time;

mod tracing;
pub use tracing::setup as setup_tracing;

mod cli;
pub use cli::{parse_duration, PortRange};
