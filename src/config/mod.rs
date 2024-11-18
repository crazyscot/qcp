// (c) 2024 Ross Younger
//! # Configuration management
//!
//! qcp obtains run-time configuration from the following sources, in order:
//! 1. Command-line options
//! 2. The user's configuration file (typically `~/.qcp.toml`)
//! 3. The system-wide configuration file (typically `/etc/qcp.toml`)
//! 4. Hard-wired defaults
//!
//! Each option may appear in multiple places, but only the first match is used.
//!
//! **Note** Configuration file locations are platform-dependent.
//! To see what applies on the current platform, run `qcp --config-files`.
//!
//! ## File format
//!
//! Configuration files use the [TOML](https://toml.io/en/) format.
//! This is a textual `key=value` format that supports comments.
//!
//! **Note** Strings are quoted; booleans and integers are not. For example:
//!
//! ```toml
//! rx="5M" # we have 40Mbit download
//! tx=1000000 # we have 8Mbit upload; we could also have written this as "1M"
//! rtt=150 # servers we care about are an ocean away
//! congestion="bbr" # this works well for us
//! ```
//!
//! ## Configurable options
//!
//! The full list of supported fields is defined by [Configuration].

mod structure;
pub use structure::Configuration;
pub(crate) use structure::Configuration_Optional;

mod manager;
pub use manager::Manager;
