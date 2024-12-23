// (c) 2024 Ross Younger
//! # Configuration management
//!
//! qcp obtains run-time configuration from the following sources, in order:
//! 1. Command-line options
//! 2. The user's configuration file (typically `~/.qcp.conf`)
//! 3. The system-wide configuration file (typically `/etc/qcp.conf`)
//! 4. Hard-wired defaults
//!
//! Each option may appear in multiple places, but only the first match is used.
//!
//! **Note** Configuration file locations are platform-dependent.
//! To see what applies on the current platform, run `qcp --config-files`.
//!
//! ## File format
//!
//! Configuration files use the same format as OpenSSH configuration files.
//! This is a textual `Key Value` format that supports comments.
//!
//! qcp supports `Host` directives with wildcard matching, and `Include` directives.
//! This allows you to tune your configuration for a range of network hosts.
//!
//! ### Example
//!
//! ```text
//! Host old-faithful
//! # This is an old server with a very limited CPU which we do not want to overstress
//! rx 125k
//! tx 0
//!
//! Host *.internal.corp
//! # This is a nearby data centre which we have a dedicated 1Gbit connection to.
//! # We don't need to use qcp, but it's convenient to use one tool in our scripts.
//! rx 125M
//! tx 0
//! rtt 10
//!
//! # For all other hosts, try to maximise our VDSL
//! Host *
//! rx 5M          # we have 40Mbit download
//! tx 1000000     # we have 8Mbit upload; we could also have written this as "1M"
//! rtt 150        # most servers we care about are an ocean away
//! congestion bbr # this works well for us
//! ```
//!
//! ## Configurable options
//!
//! The full list of supported fields is defined by [Configuration].
//!
//! On the command line:
//! * `qcp --show-config` outputs a list of supported fields, their current values, and where each value came from.
//! * For an explanation of each field, refer to `qcp --help` .
//! * `qcp --config-files` outputs the list of configuration files for the current user and platform.
//!
//! ### Traps and tips
//! 1. Like OpenSSH, for each setting we use the value from the _first_ Host block we find that matches the remote hostname.
//! 1. Each setting is evaluated independently.
//! - In the example above, the `Host old-faithful` block sets an `rx` but does not set `rtt`. Any operations to `old-faithful` inherit `rtt 150` from the `Host *` block.
//! 1. The `tx` setting has a default value of 0, which means "use the active rx value". If you set `tx` in a `Host *` block, you probably want to set it explicitly everywhere you set `rx`.
//!
//! If you have a complicated config file we recommend you structure it as follows:
//! * Any global settings that are intended to apply to all hosts
//! * `Host` blocks; if you use wildcards, from most-specific to least-specific
//! * A `Host *` block to provide default settings to apply where no more specific value has been given
//!

mod structure;
pub use structure::Configuration;
pub(crate) use structure::Configuration_Optional;

mod manager;
pub use manager::Manager;

pub(crate) const BASE_CONFIG_FILENAME: &str = "qcp.conf";

pub(crate) mod ssh;
