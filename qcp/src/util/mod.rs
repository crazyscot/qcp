// QCP general utility code that didn't fit anywhere else
// (c) 2024 Ross Younger

mod dns;
pub use dns::{lookup_host_by_family, AddressFamily};

mod io;
pub use io::{open_file_read, open_file_write};

mod tracing;
pub use tracing::setup_tracing;
