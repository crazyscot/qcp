//! General utility code that didn't fit anywhere else
// (c) 2024 Ross Younger

mod address_family;
pub use address_family::AddressFamily;

mod dns;
pub use dns::lookup_host_by_family;

mod cert;
pub use cert::Credentials;

pub mod io;
pub mod socket;
pub mod stats;
pub mod time;

mod tracing;
pub(crate) use tracing::trace_level;
pub use tracing::{TimeFormat, is_initialized as tracing_is_initialised, setup as setup_tracing};

mod port_range;
pub use port_range::PortRange;

mod optionalify;
pub use optionalify::{derive_deftly_template_Optionalify, insert_if_some};

#[cfg(test)]
pub(crate) fn make_test_tempfile(
    data: &str,
    filename: &str,
) -> (std::path::PathBuf, tempfile::TempDir) {
    let tempdir = tempfile::tempdir().unwrap();
    let path = tempdir.path().join(filename);
    std::fs::write(&path, data).expect("Unable to write tempfile");
    (path, tempdir)
}
