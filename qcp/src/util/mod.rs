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

mod file_ext;
pub(crate) use file_ext::FileExt;
mod metadata_ext;
pub(crate) use metadata_ext::FsMetadataExt;

pub(crate) mod io;
pub(crate) mod process;
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

mod vec_or_string;
pub use vec_or_string::VecOrString;
