// (c) 2024 Ross Younger

//! ## 🕵️ Troubleshooting
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
//!
//! ### qcp isn't using the network parameters you expected it to
//! * Parameters specified on the command line always override those in config files.
//! * Settings in the user config file take precedence over those in the system config file.
//! * For each setting, the first value found in a matching Host block wins.
//! * If both server and client specify a setting, the logic to combine them is described in
//!   [`combine_bandwidth_configurations`](crate::transport::combine_bandwidth_configurations).
//! * See also [configuration debug tools](#configuration-debug-tools).
//!
//! ## Debug tools
//! * `--debug` enables various debug chatter.
//! * You can also set RUST_LOG in the environment. For example, `RUST_LOG=qcp=trace`.
//! * `--remote-debug` asks the server to send its debug output back over the ssh connection.
//!   _Be cautious when doing so we the additional traffic will likely interfere with transfer speed._
//! * If you particularly want to set RUST_LOG on the server process, you'll need to configure
//!   the ssh client to send this (`SendEnv`) and the server to allow it (`SetEnv`).
//!
//! ### Configuration debug
//! * `--config-files` will tell you where, on the current platform, qcp is looking for configuration files.
//! * The `--dry-run` mode reports the negotiated network configuration that we would have used for a given proposed transfer.
//! ```text
//! 2025-02-10 09:32:07Z  INFO Negotiated network configuration: rx 37.5MB (300Mbit),
//! tx 12.5MB (100Mbit), rtt 300ms, congestion algorithm Cubic with initial window <default>
//! ```
//! * If you want to see the server's idea of the configuration, you'll find that in `--remote-debug` mode. _Note that tx and rx are
//!   the opposite way round, from from the server's point of view!_
//! ```text
//! ...
//! 2025-02-10 09:32:06L DEBUG Server: Final network configuration: rx 12.5MB (100Mbit), tx 37.5MB (300Mbit),
//! rtt 300ms, congestion algorithm Cubic with initial window <default>
//! ...
//! ```
//! * Add `--show-config` to your command line to see the client settings qcp would use and where it got them from.
//!   The output looks something like this:
//! ```text
//! $ qcp --show-config server234:some-file /tmp/
//! ┌─────────────────────────┬─────────────┬───────────────────────────────┐
//! │ field                   │ value       │ source                        │
//! ├─────────────────────────┼─────────────┼───────────────────────────────┤
//! │ (Remote host)           │ server234   │                               │
//! │ AddressFamily           │ any         │ default                       │
//! │ Congestion              │ Cubic       │ default                       │
//! │ InitialCongestionWindow │ 0           │ default                       │
//! │ Port                    │ 10000-12000 │ /home/xyz/.qcp.conf (line 26) │
//! │ RemotePort              │ 0           │ default                       │
//! │ Rtt                     │ 300         │ default                       │
//! │ Rx                      │ 37M5        │ /home/xyz/.qcp.conf (line 22) │
//! │ Ssh                     │ ssh         │ default                       │
//! │ SshConfig               │ []          │ default                       │
//! │ SshOptions              │ []          │ default                       │
//! │ TimeFormat              │ UTC         │ /etc/qcp.conf (line 14)       │
//! │ Timeout                 │ 5           │ default                       │
//! │ Tx                      │ 12M5        │ /home/xyz/.qcp.conf (line 23) │
//! └─────────────────────────┴─────────────┴───────────────────────────────┘
//! ```
//!
//! * Add `--remote-config` to the command line to have the server report its settings.
//!   These come in two blocks, the _static_ configuration and the _final_ configuration after applying system defaults and client preferences.
//! ```text
//! 2025-02-10 09:26:02Z  INFO Server: Static configuration:
//! ┌───────────────┬─────────────┬───────────────────────────────┐
//! │ field         │ value       │ source                        │
//! ├───────────────┼─────────────┼───────────────────────────────┤
//! │ (Remote host) │ 10.22.55.77 │                               │
//! │ Port          │ 10000-12000 │ /home/xyz/.qcp.conf (line 26) │
//! │ Rtt           │ 1           │ /home/xyz/.qcp.conf (line 19) │
//! │ Rx            │ 125M        │ /home/xyz/.qcp.conf (line 17) │
//! │ TimeFormat    │ UTC         │ /home/xyz/.qcp.conf (line 25) │
//! │ Tx            │ 125M        │ /home/xyz/.qcp.conf (line 18) │
//! └───────────────┴─────────────┴───────────────────────────────┘
//! 2025-02-10 09:26:02Z  INFO Server: Final configuration:
//! ┌─────────────────────────┬─────────────┬───────────────────────────────┐
//! │ field                   │ value       │ source                        │
//! ├─────────────────────────┼─────────────┼───────────────────────────────┤
//! │ (Remote host)           │ 10.22.55.77 │                               │
//! │ AddressFamily           │ any         │ default                       │
//! │ Congestion              │ Cubic       │ default                       │
//! │ InitialCongestionWindow │ 0           │ default                       │
//! │ Port                    │ 10000-12000 │ /home/xyz/.qcp.conf (line 26) │
//! │ RemotePort              │ 0           │ default                       │
//! │ Rtt                     │ 1           │ /home/xyz/.qcp.conf (line 19) │
//! │ Rx                      │ 123M        │ config resolution logic       │
//! │ Ssh                     │ ssh         │ default                       │
//! │ SshConfig               │ []          │ default                       │
//! │ SshOptions              │ []          │ default                       │
//! │ TimeFormat              │ UTC         │ /home/xyz/.qcp.conf (line 25) │
//! │ Timeout                 │ 5           │ default                       │
//! │ Tx                      │ 125M        │ /home/xyz/.qcp.conf (line 18) │
//! └─────────────────────────┴─────────────┴───────────────────────────────┘
//! ```
