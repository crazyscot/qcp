// QCP general utility code that didn't fit anywhere else
// (c) 2024 Ross Younger

mod dns;
pub use dns::{lookup_host_by_family, AddressFamily};

/// File I/O utilities
pub mod io;
/// Socket utilities
pub mod socket;
/// Statistics processing and output
pub mod stats;
/// Time utilities
pub mod time;

/// Tracing helpers
mod tracing;
pub use tracing::setup as setup_tracing;

mod cli;
pub use cli::PortRange;
