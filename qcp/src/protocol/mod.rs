// (c) 2024 Ross Younger
// Code in this module: Protocol defininitions
//!
#![allow(clippy::doc_markdown)]
//! # 📖 The QCP protocol
//! `qcp` is a **hybrid protocol**.
//! The binary contains the complete protocol implementation,
//! except for the ssh binary used to establish the control channel itself.
//!
//! The protocol flow looks like this:
//!
//! 1. The user runs `qcp` from the machine we will call the _initiator_ or _client_.
//!    * qcp uses ssh to connect to the _remote_ machine and start a `qcp --server` process there.
//!    * We call this link between the two processes the _control channel_.
//!    * The _remote_ machine is also known as the _server_, in keeping with other communication protocols.
//! 1. Both sides generate ephemeral self-signed TLS certificates.
//! 1. The remote machine binds to a UDP port and sets up a [QUIC] _endpoint_.
//! 1. The two machines exchange messages over the [control] channel containing:
//!    * cryptographic identities
//!    * server UDP port
//!    * bandwidth configuration and any resulting warning
//! 1. The initiator opens up a QUIC connection to the remote.
//!    * N.B. While UDP is a connectionless protocol, QUIC provides connection semantics, with multiple bidirectional _streams_ possible on top of a connection between two endpoints.)
//! 1. For each file to be transferred in either direction, the initiator opens a QUIC _stream_ over the existing connection.
//!    * We call this a _session_.
//!    * The two endpoints use the [session] protocol to move data to where it needs to be.
//! 1. When all is said and done, the initiator closes the control channel. This leads to everything being torn down.
//!
//! ## Motivation
//!
//! This protocol exists because I needed to copy multiple large (3+ GB) files from
//! a server in Europe to my home in New Zealand.
//!
//! I've got nothing against `ssh` or `scp`. They're brilliant. I've been using them since the 1990s.
//! However they run on top of [TCP], which does not perform very well when the network is congested.
//! With a fast fibre internet connection, a long round-trip time and noticeable packet
//! loss, I was right in the sour spot.
//! TCP did its thing and slowed down, but when the congestion cleared it was very slow to
//! get back up to speed.
//!
//! If you've ever been frustrated by download performance from distant websites,
//! you might have been experiencing this same issue.
//! Friends with satellite (pre-Starlink) internet connections seem to be particularly badly affected.
//!
//! ## Security design 🛡️
//!
//! The security goals for this project are fairly straightforward:
//!
//! - Only authenticated users can transfer files to/from a system
//! - Data in transit should be kept confidential, with its authenticity and integrity protected; all of this by well-known, reputable cryptographic algorithms
//! - **Security of data at rest at either end is out of scope**, save for the obvious requirement that the copied file be put where the user wanted us to put it
//! - _I do not want to write my own cryptography or user authentication_
//! - _I do not want to rely on PKI if I can help it_
//!
//! [ssh] includes a perfectly serviceable, well understood and battle-tested user authentication system.
//! Sysadmins can set their own policies regarding password, cryptographic or other authentication methods.
//!
//!
//! [QUIC] traffic is protected by [TLS]. In many cases, a QUIC server would have a TLS certificate
//! signed by a [CA] in the same way as a website.
//!
//! However, I wanted bidirectional endpoint authentication. I also didn't want the hassle of setting
//! up and maintaining certificates at both ends. ([LetsEncrypt] is great for many things,
//! but not so useful in this case; I don't want to run a web server on my home net connection.)
//!
//! After some thought I realised that the solution lay in a hybrid, bootstrapping protocol.
//! * Each endpoint generates a fresh, ephemeral TLS key every time.
//! * With ssh connecting the two endpoints, we have an easy way to ensure that TLS
//!   credentials genuinely belong to the other end.
//!
//! ### Results
//!
//! The endpoints will only establish a connection:
//!
//! * to one specific TLS instance;
//! * identified by a self-signed certificate that it just received over the control channel, which is assumed secure;
//! * confirmed by use of a private key that only the other endpoint knows (having just generated it).
//!
//! Therefore, data remains secure in transit provided:
//!
//! * the ssh and TLS protocols themselves have not been compromised
//! * your credentials to log in to the remote machine have not been compromised
//! * the random number generators on both endpoints are of sufficient quality
//! * nobody has perpetrated a software supply chain attack on qcp, ssh, or any of the myriad components they depend on
//!
//! ## Prior Art
//!
//! * [FASP](https://en.wikipedia.org/wiki/Fast_and_Secure_Protocol) is a high-speed data transfer protocol that runs on UDP.
//!   It is proprietary and patented; the patents are held by [Aspera](http://ibm.com/aspera/) which was acquired by IBM.
//! * [QUIC] was invented by a team at Google in 2012, and adopted as a standard by the IETF in 2016.
//!   The idea is simple: your data travels over UDP instead of TCP.
//!   * Obviously, you lose the benefits of TCP (reliability, packet sequencing, flow control), so you have to reimplement those.
//!     While TCP is somewhat ossified, the team behind QUIC picked and chose the best bits and changed its shape.
//! * [quinn](https://docs.rs/quinn/latest/quinn/), a Rust implementation of QUIC
//! * [quicfiletransfer](https://github.com/sirgallo/quicfiletransfer) uses [QUIC] to transfer files, but without an automated control channel.
//!
//! ## See Also
//! * [RFC 9000 "QUIC: A UDP-Based Multiplexed and Secure Transport"](https://www.rfc-editor.org/rfc/rfc9000.html)
//! * [RFC 9001 "Using TLS to Secure QUIC"](https://www.rfc-editor.org/rfc/rfc9001.html)
//! * [RFC 9002 "QUIC Loss Detection and Congestion Control"](https://www.rfc-editor.org/rfc/rfc9002.html)
//! * [quinn comparison of TCP, UDP and QUIC](https://quinn-rs.github.io/quinn/)
//!
//! [QUIC]: <https://quicwg.github.io/>
//! [ssh]: <https://en.wikipedia.org/wiki/Secure_Shell>
//! [TCP]: <https://en.wikipedia.org/wiki/Transmission_Control_Protocol>
//! [TLS]: <https://en.wikipedia.org/wiki/Transport_Layer_Security>
//! [CA]: <https://en.wikipedia.org/wiki/Certificate_authority>
//! [LetsEncrypt]: <https://letsencrypt.org/>

pub mod common;
pub mod compat;
pub mod control;
pub mod session;

#[cfg(any(test, feature = "unstable-test-helpers"))]
#[cfg_attr(coverage_nightly, coverage(off))]
pub(crate) mod test_helpers;
