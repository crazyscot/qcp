// (c) 2024 Ross Younger

//! 🕵️ Troubleshooting
//!
//! ## General
//!
//! The `--debug` and `--remote-debug` options report information that may help you diagnose issues.
//!
//! This program also understands the `RUST_LOG` environment variable which might let you probe deeper.
//! Some possible settings for this variable are:
//!
//! * `qcp=trace` outputs tracing-level output from this crate
//! * `trace` sets all the Rust components to trace mode, which includes an _awful lot_ of output from quinn (the QUIC implementation).
//!
//! Note that this variable setting applies to the local machine, not the remote. If you arrange to set it on the remote, the output will come back over the ssh channel; **this may impact performance**.
//!
//! ### You can't ssh to the remote machine
//!
//! Sorry, that's a prerequisite. Get that working first, then come back to qcp.
//!
//! qcp calls ssh directly; ssh will prompt you for a password and may invite you to verify the remote host key.
//!
//! ### The QUIC connection times out
//!
//! * Does the remote host firewall inbound UDP connections?
//!   If so, you will need to allocate and open up a small range of inbound ports for use by qcp.
//!   Use the `--remote-port` option to tell it which.
//! * Is the remote host behind NAT? Sorry, NAT traversal is not currently supported.
//!   At best, you might be able to open up a small range of UDP ports on the NAT gateway which are directly forwarded to the target machine.
//!   Use the `--remote-port` option to tell it which.
//! * Are outbound UDP packets from the initiator firewalled?
//!   You will need to open up some outbound ports; use the `--port` option to tell qcp which.
//!
//! ### Performance is poor?
//!
//! (This became a separate doc. See [performance](super::performance).)
//!
//! ### Excess bandwidth usage
//!
//! This utility is designed to soak up all the bandwidth it can.
//!
//! When there is little packet loss, the overhead is minimal (2-3%). However when packets do go astray, the retransmits can add up. If you use the BBR congestion controller, this will add up much faster as it tries to keep the pipe fuller; I've seen it report over 20% packet loss.
//!
//! If you're on 95th percentile billing, you may need to take this into account. But if you are on that sort of deal, you are hopefully already spending time to understand and optimise your traffic profile.
//!
//! ### Using qcp interferes with video calls / Netflix / VOIP / etc
//!
//! This utility is designed to soak up all the bandwidth it can.
//!
//! QUIC packets are UDP over IP, the same underlying protocol used for streaming video, calls and so forth.
//! They are quite literally competing with any A/V data you may be running.
//!
//! If this bothers you, you might want to look into setting up QoS on your router.
//!
//! There might be some mileage in having qcp try to limit its bandwidth use or tune it to be less aggressive in the face of congestion, but that's not implemented at the moment.
//!
//! ### It takes a long time to set up the control channel
//!
//! The control channel is an ordinary ssh connection, so you need to figure out how to make ssh faster.
//! This is not within qcp's control.
//!
//! * Often this is due to a DNS misconfiguration at the server side, causing it to stall until a DNS lookup times out.
//! * There are a number of guides online purporting to advise you how to speed up ssh connections; I can't vouch for them.
//! * You might also look into ssh connection multiplexing.
