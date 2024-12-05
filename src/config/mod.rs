//! Configuration management
// (c) 2024 Ross Younger

mod structure;
pub use structure::Configuration;
pub(crate) use structure::Configuration_Optional;

mod manager;
pub use manager::Manager;
