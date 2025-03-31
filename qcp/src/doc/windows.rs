// (c) 2025 Ross Younger

//! 🪟 Notes for Windows users
//!
//! # 🖥️ Running qcp from the command line (Client mode)
//!
//! This worked for me, straight out of the box.
//!
//! # 🖥 Connecting qcp to a server running Windows (Server mode)
//!
//! ## 1. Get ssh going first
//!
//! I tested on Windows 11 with OpenSSH Server (installed via System -> Optional features).
//! See <https://learn.microsoft.com/en-us/windows-server/administration/OpenSSH/openssh-server-configuration> for details.
//!
//! I found I needed to manually start sshd (in Services).
//!
//! I found I needed to explicitly allow access through the Windows firewall for sshd before I could connect to it.
//!
//! ## 2. Set up qcp somewhere the ssh daemon can reach it
//!
//! You need to put the qcp executable somewhere that the ssh server can find it on a non-interactive login.
//! The most convenient place I found was in my profile directory `C:\Users\myusername`.
//!
//! ssh subsystem mode may be an option, but I couldn't readily get that working.
//!
//! ## 3. Allow access through the Windows firewall
//!
//! You need to allow qcp through the Windows firewall.
//! If you don't, the machine initiating the connection will complain of a protocol
//! timeout setting up the QUIC session.
//!
//! This is in Windows Security > Firewall & network protection > Allow an app through firewall.
//!
//! # 🚀 Network tuning
//!
//! qcp performed well straight out of the box; no additional system configuration was necessary.
//! Nevertheless, we still check the buffer sizes at runtime, to be able to warn if this isn't the case in future.
//!
//! # 🕵️ Troubleshooting
//!
//! ### Protocol timeout setting up the QUIC session
//!
//! Most likely the Windows firewall, or some other intervening firewall, is blocking the
//! UDP packets. [See above](#3-allow-access-through-the-windows-firewall).
