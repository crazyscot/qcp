// (c) 2024 Ross Younger
//! # 📖 Configuration management
//!
//! qcp obtains run-time configuration from the following sources, in order:
//! 1. Command-line options
//! 2. The user's configuration file on either side of the connection (see [negotiation](#configuration-negotiation))
//!    * On Unix, this is `~/.qcp.conf` or `~/.config/qcp/qcp.conf`
//!    * On Windows, this is `%AppData%\Roaming\qcp\qcp.conf`
//! 3. The system-wide configuration file on either side of the connection
//!    * On Unix, this is `/etc/qcp.conf`
//!    * On Windows, this is `%ProgramData%\qcp.conf`
//! 4. Hard-wired defaults
//!
//! Run `qcp --config-files` for a list of which files we read.
//!
//! Each option may appear in multiple places, but only the first match is used.
//!
//! ## File format
//!
//! qcp uses the same basic format as OpenSSH configuration files.
//!
//! Options may be specified in any order.
//!
//! Each line contains a keyword (the option name) and one or more arguments.
//!
//! Option names are single words. They are case insensitive; hyphens and underscores are ignored.
//! In other words, you can use UPPERCASE, lowercase, camelCase, mIxEDcAse, SHOUTY_SNAKE_CASE, kebab-case, Train-Case, whatever you like.
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
//! * The qcp client process matches against the remote host given on the QCP command line, before DNS or alias resolution.
//!   It does _not_ resolve hostname to IP address. However, if you connect to hosts by IP address, patterns (for example `10.11.12.*`) work in the obvious way.
//! * The remote (server) process matches the IP address passed to it by the ssh server in the `SSH_CONNECTION` or `SSH_CLIENT` environment variables.
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
//! The set of supported fields is the [Configuration] structure.
//!
//! In configuration files, option keywords are case insensitive and ignore hyphens and underscores.
//! (On the command line, they must be specified in kebab-case.)
//!
//! * `qcp --show-config` outputs a list of supported fields, their current values, and where each value came from.
//! * For an explanation of each field, refer to `qcp --help` .
//! * `qcp --config-files` outputs the list of configuration files for the current user and platform.
//!
//! ## Configuration negotiation
//!
//! The remote qcp ("server") process is on another machine, and can have its own set of user and system-wide configuration files.
//! Options governing the transport configuration (bandwidth, round-trip time, UDP ports) may be specified by either side, and are
//! combined at runtime.
//!
//! The idea is that you configure either side for the bandwidth available to it, and qcp then uses the lower of the two.
//! If you're working from a laptop away from your usual connection, you might run a speed test and tell qcp the results.
//!
//! The logic used to combine configurations is described under [`combine_bandwidth_configurations`](crate::transport::combine_bandwidth_configurations).
//!
//! ## Example
//!
//! ```text
//! Host old-faithful
//! # This is an old server with a very limited CPU which we do not want to overstress.
//! # old-faithful isn't its DNS name; it's a hostname aliased in ssh_config.
//! rx 125k  # 1 Mbit limit. Yes, it's a really old server.
//! tx 0     # tx 0 means "same as rx"
//! # This server runs a tight firewall; inbound UDP is only allowed on certain ports.
//! RemotePort 65400-65500
//!
//! Host *.internal.corp 172.31.200.*
//! # This is a nearby data centre which we have a dedicated 1Gbit connection to.
//! # We don't need to use qcp, but it's convenient to use one tool in our scripts.
//! # We specify the group both by domain name and netblock: the qcp client process
//! # matches against whatever you give on the command line, and the qcp server
//! # uses only the connecting IP address.
//! # (IPv6 addresses would work here too.)
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
//! 1. The qcp client process does NOT resolve hostname to IP address when determining which `Host` block(s) to match.
//!    This is consistent with OpenSSH.
//!    * However, the qcp server process ONLY matches against the IP address passed to it by the OpenSSH server.
//!    * Therefore, in an environment which may act as both qcp client and server, you may need to specify options by both hostname and netblock.
//!
//! ## Building a configuration
//! We suggest the following approach to setting up a configuration file.
//!
//! 1. Set up a `Host *` block specifying `Tx` and `Rx` suitable for your local network uplink.
//!    * In a data centre environment, the bandwidth limits will likely be whatever your network interface is capable of.
//!      (If the data centre has limited bandwidth, or your contract specifies something lower, use that instead.)
//!    * In a host connected to a standard ISP connection, the bandwidth limits will be whatever you're paying your ISP for.
//!      If you're not sure, you might use speedtest.net or a similar service.
//! 1. Make a best-guess to what the Round Trip Time might be in the default case, and add this to `Host *`.
//!    If you mostly deal with servers on the same continent as you, this might be somewhere around 50 or 100ms.
//!    If you mostly deal with servers on the other side of the planet, this might be 300s or even more.
//! 1. Add any other global options to the `Host *` block
//!    1. If this machine will act as a qcp server and has a firewall that limits incoming UDP traffic, set up a firewall exception on a range of ports and configure that as `port`.
//!    1. Set up any non-standard `ssh`, `ssh_options` or `ssh_config` that may be needed.
//!    1. If you want to use UTC when printing messages, set `TimeFormat`.
//! 1. If there are any specific hosts or network blocks that merit different network settings, add `Host` block(s) for them as required.
//!    Order these from most-specific to least-specific.
//!    Be sure to place them _above_ `Host *` in the config file.
//! 1. Try it out! Copy some files around and see what network performance is like.
//!    Note that these files need to be large (hundreds of MB or more) to reach peak performance.
//!    * Use `--dry-run` mode to preview the final network configuration for a proposed file transfer.
//!      If the output isn't what you expected, use `--remote-config` to see where the various settings came from.
//!    * See also the [performance notes](crate::doc::performance) and [troubleshooting](crate::doc::troubleshooting).
//!

pub(crate) mod structure;
pub(crate) use structure::Configuration;
pub(crate) use structure::Configuration_Optional;

mod clicolor;
use clicolor::Env as ClicolorEnv;

mod sysdefault;
use sysdefault::SystemDefault;

mod manager;
pub use manager::Manager;

mod prettyprint;

pub(crate) const BASE_CONFIG_FILENAME: &str = "qcp.conf";

mod source;
pub(crate) use source::LocalConfigSource as Source;
pub(crate) mod ssh;

pub use crate::cli::styles::ColourMode;
pub use ssh::includes::find_include_files;
