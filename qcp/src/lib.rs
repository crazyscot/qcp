// (c) 2024 Ross Younger

#![allow(clippy::doc_markdown)]
//! The QUIC Copier (`qcp`) is an experimental high-performance remote file copy utility,
//! intended for long-distance high-bandwidth internet connections ("long, fat pipes").
//!
//! ## 📖 Documentation
//!
//! * [About the QCP protocol](protocol)
//! * [Configuring QCP](config)
//! * [Performance tuning](doc::performance)
//! * [Troubleshooting](doc::troubleshooting)
//! * OS-specific notes:
//!   * 🍎 [OSX](os::osx)
//!   * 🐧 [Linux & other Unix](os::unix)
//!   * 🪟 [Windows](os::windows)
//!
//! ## Overview
//! - 🔧 Drop-in replacement for `scp`
//! - 🛡️ Similar security to `scp`, using well-known and trustworthy mechanisms
//!   - User authentication uses `ssh` to establish a control channel and exchange TLS certificates. No PKI is necessary.
//!   - Data in transit is protected by TLS, with strict certificate checks in both directions
//! - 🚀 Better throughput on congested networks
//!   - Data is transported using the [QUIC](https://quicwg.github.io/) protocol over UDP
//!   - Tunable network properties
//!
//! #### Use case
//!
//! This utility and protocol can be useful when copying **large** files (tens of MB or more),
//! from _point to point_ over a _long, fat, congested pipe_.
//!
//! I was inspired to write this when I needed to copy a load of multi-GB files from a server on the other side of the planet.
//!
//! #### Limitations
//! - You must be able to ssh directly to the remote machine, and exchange UDP packets with it on a given port. (If the local machine is behind connection-tracking NAT, things work just fine. This is the case for the vast majority of home and business network connections. If need be, you can configure qcp to use a particular port range.)
//! - Network security systems can't readily identify QUIC traffic as such. It's opaque, and high bandwidth. Some security systems might flag it as a potential threat.
//!
//! #### What qcp is not
//!
//! * A way to serve files to the public (Use http3.)
//! * A way to speed up downloads from sites you do not control (It's up to whoever runs those sites to install http3 or set up a [CDN].)
//! * Peer to peer file transfer (Use [BitTorrent]?)
//! * An improvement for interactive shells (Use [mosh].)
//! * Delta-based copying (Use [rsync].)
//!
//! ## 🧰 Getting Started
//!
//! * You must have ssh access to the target machine.
//! * You must also be able to connect to the target on a given UDP port.
//!   - If the local machine is behind connection-tracking NAT, things usually work just fine. This is the case for the vast majority of home and business network connections.
//!   - If the target is behind a firewall, you need to configure the firewall so at least some small set of UDP ports is
//!     accessible, and **not subject to network port translation**.
//!     (You can configure qcp to use a particular port range if you need to; see `--port` / `--remote-port`.)
//!     - In different terms: The target machine will bind to a random UDP port, and advise its peer (via ssh)
//!       of its choice of port number.
//! * Install the `qcp` binary on both machines. It needs to be in your `PATH` on the remote machine,
//!   or you need to set up `SshSubsystem` mode.
//!   * Check the platform-specific notes: [OSX](https://docs.rs/qcp/latest/qcp/doc/osx/index.html),
//!     [Linux/other Unix](https://docs.rs/qcp/latest/qcp/doc/unix/index.html),
//!     [Windows](https://docs.rs/qcp/latest/qcp/doc/windows/index.html)
//! * Try it out! Use `qcp` where you would `scp`, e.g. `qcp myfile some-server:some-directory/`
//! * Browse the tuning options in [Configuration](https://docs.rs/qcp/latest/qcp/struct.Configuration.html)
//! * Set up a [config](config) file that tunes for your network connection. (You might find the `--stats` option useful when experimenting.)
//!
//! ## 📖 How it works
//!
//! The brief version:
//! 1. We ssh to the remote machine and run `qcp --server` there (with no further args, i.e. you can use `command="qcp --server"` in your authorized_keys file)
//! 1. Both sides generate a TLS key and exchange self-signed certs over the ssh pipe between them
//! 1. We use those certs to set up a QUIC session between the two
//! 1. We transfer files over QUIC
//!
//! The [protocol] documentation contains more detail and a discussion of its security properties.
//!
//! * **qcp uses the ssh binary on your system to connect to the target machine**.
//!   ssh will check the remote host key and prompt you for a password or passphrase in the usual way.
//! * **qcp will read your ssh config file** to resolve any Hostname aliases you may have defined there.
//!   The idea is, if you can `ssh` to a host, you should also be able to `qcp` to it.
//!   However, some particularly complicated ssh config files may be too much for qcp to understand.
//!   (In particular, `Match` directives are not currently supported.)
//!   In that case, you can use `--ssh-config` to provide an alternative configuration (or set it in your qcp configuration file).
//!
//! ## Configuration
//!
//! On the command line, qcp has a comprehensive `--help` message.
//!
//! Many options can also be specified in a config file. See [config] for details.
//!
//! ### 📖 Getting the best out of qcp
//!
//! See [performance](doc::performance) and [troubleshooting](doc::troubleshooting).
//!
//! ## Miscellanea
//!
//! #### MSRV policy
//!
//! As this is an application crate, the MSRV is not guaranteed to remain stable.
//! The MSRV may be upgraded from time to time to take advantage of new language features.
//!
//! [QUIC]: https://quicwg.github.io/
//! [ssh]: https://en.wikipedia.org/wiki/Secure_Shell
//! [CDN]: https://en.wikipedia.org/wiki/Content_delivery_network
//! [BitTorrent]: https://en.wikipedia.org/wiki/BitTorrent
//! [rsync]: https://en.wikipedia.org/wiki/Rsync
//! [mosh]: https://mosh.org/
//!
//! #### Feature flags
#![doc = document_features::document_features!()]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub(crate) mod cli;
pub use cli::cli as main;
pub use cli::styles;
pub(crate) mod client;
pub(crate) use client::client_main;
pub use client::{CopyJobSpec, FileSpec, Parameters};

pub mod config;
pub use config::structure::Configuration;

pub mod control;
pub mod protocol;
mod server;
pub(crate) use server::server_main;
pub(crate) mod session;
pub mod transport;
pub mod util;

pub mod doc;

pub mod os;

mod version;

#[doc(hidden)]
pub use derive_deftly;
// Use the current version of derive_deftly here:
derive_deftly::template_export_semver_check!("0.14.0");

/// Helper functions and structs exported for qcp-unsafe-tests.
///
/// **This module should be regarded as unstable and subject to modification without notice**.
/// By keeping unsafe tests in a separate crate, we can forbid unsafe_code in qcp proper.
#[cfg(feature = "unstable-test-helpers")]
pub mod test_helpers {
    pub use crate::session::test_shared::test_getx_main;
}
