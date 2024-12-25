// (c) 2024 Ross Younger
//! # 📖 Configuration management
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
//! qcp uses the same basic format as OpenSSH configuration files.
//!
//! Each line contains a keyword and one or more arguments.
//! Keywords are single words and are case-insensitive.
//!
//! Arguments are separated from keywords, and each other, by whitespace.
//! (It is also possible to write `Key=Value` or `Key = Value`.)
//!
//! Arguments may be surrounded by double quotes (`"`); this allows you to set an argument containing spaces.
//! If a backslash, double or single quote forms part of an argument it must be backslash-escaped i.e. `\"` or `\\`.
//!
//! Empty lines are ignored.
//!
//! **qcp supports Host and Include directives in way that is intended to be compatible with OpenSSH.**
//! This allows you to tune your configuration for a range of network hosts.
//!
//! #### Host
//!
//! `Host host [host2 host3...]`
//!
//! This directive introduces a _host block_.
//! All following options - up to the next `Host` - only apply to hosts matching any of the patterns given.
//!
//! * Pattern matching uses `*` and `?` as wildcards in the usual way.
//! * A single asterisk `*` matches all hosts; this is used to provide defaults.
//! * A pattern beginning with `!` is a _negative_ match; it matches all remote hosts _except_ those matching the rest of the pattern.
//! * Pattern matching is applied directly to the remote host given on the QCP command line, before DNS or alias resolution.
//!   If you connect to hosts by IP address, a pattern of `10.11.12.*` works in the obvious way.
//!
//! #### Include
//!
//! `Include file [file2 file3...]`
//!
//! Include the specified file(s) in the configuration at the current point.
//!
//! * Glob wildcards ('*' and '?') are supported in filenames.
//! * User configuration files may refer to pathnames relative to '~' (the user's home directory).
//! * Filenames with relative paths are assumed to be in `~/.ssh/` if read from a user configuration file, or `/etc/ssh/` if read from a system configuration file.
//! * An Include directive inside a Host block retains the Host context.
//!   This may be useful to apply common directives to multiple hosts with minimal repetition.
//!   Note that if an included file begins a new Host block, that will continue to apply on return to the including file.
//! * It is possible for included files to themselves include additional files; there is a brake that prevents infinite recursion.
//!
//! ## Configurable options
//!
//! The set of supported fields is defined by [Configuration].
//!
//! In configuration files, option keywords are case insensitive and ignore hyphens and underscores.
//! (On the command line, they must be specified in kebab-case.)
//!
//! * `qcp --show-config` outputs a list of supported fields, their current values, and where each value came from.
//! * For an explanation of each field, refer to `qcp --help` .
//! * `qcp --config-files` outputs the list of configuration files for the current user and platform.
//!
//! ## Example
//!
//! ```text
//! Host old-faithful
//! # This is an old server with a very limited CPU which we do not want to overstress
//! rx 125k
//! tx 0
//! RemotePort 65400-65500 # allowed in firewall config
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
//! ## Tips and traps
//! 1. Like OpenSSH, for each setting we use the value from the _first_ Host block we find that matches the remote hostname.
//! 1. Each setting is evaluated independently.
//!    In the example above, the `Host old-faithful` block sets an `rx` but does not set `rtt`.
//!    Any operations to `old-faithful` therefore inherit `rtt 150` from the `Host *` block.
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
