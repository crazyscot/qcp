//! General utility code that didn't fit anywhere else
//!
//! Note that most of this module is not exported.
// (c) 2024 Ross Younger

mod address_family;
pub use address_family::AddressFamily;

mod dns;
pub(crate) use dns::lookup_host_by_family;

mod cert;
pub(crate) use cert::Credentials;

pub(crate) mod io;
pub(crate) mod socket;
pub(crate) mod stats;
pub(crate) mod time;

pub(crate) mod serialization;
pub use serialization::SerializeAsString;

mod tracing;
pub(crate) use tracing::{
    ConsoleTraceType, SetupFn as TracingSetupFn, TimeFormat,
    is_initialized as tracing_is_initialised, setup as setup_tracing, trace_level,
};

mod port_range;
pub use port_range::PortRange;

mod optionalify;
pub use optionalify::{derive_deftly_template_Optionalify, insert_if_some};

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod test_protocol;

mod vec_or_string;
pub use vec_or_string::VecOrString;
