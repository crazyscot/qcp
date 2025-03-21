// (c) 2024 Ross Younger

#![allow(clippy::doc_markdown)]
//! The QUIC Copier (`qcp`) is an experimental high-performance remote file copy utility,
//! intended for long-distance internet connections.
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
//! ### Use case
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
//! Many options can also be specified in a config file. See [config] for detalis.
//!
//! ## 📈 Getting the best out of qcp
//!
//! See [performance](doc::performance) and [troubleshooting](doc::troubleshooting).
//!
//! ## MSRV policy
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
//! ## Feature flags
#![doc = document_features::document_features!()]
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod cli;
pub(crate) mod client;
pub(crate) use client::client_main;
pub use client::{CopyJobSpec, FileSpec, Parameters};

pub mod config;
pub use config::structure::Configuration;

pub(crate) mod control;
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
