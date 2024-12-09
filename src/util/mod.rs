//! General utility code that didn't fit anywhere else
// (c) 2024 Ross Younger

mod address_family;
pub use address_family::AddressFamily;

mod dns;
pub use dns::lookup_host_by_family;

mod cert;
pub use cert::Credentials;

pub mod cli;

pub mod humanu64;
pub mod io;
pub mod socket;
pub mod stats;
pub mod time;

mod tracing;
pub use tracing::{setup as setup_tracing, TimeFormat};

mod port_range;
pub use port_range::PortRange;

mod optionalify;
pub use optionalify::{derive_deftly_template_Optionalify, insert_if_some};
