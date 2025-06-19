// (c) 2025 Ross Younger

//! 🍎 qcp on OSX
//!
//! This mostly just worked, using the [Unix](super::UnixPlatform) platform implementation.
//! Running the qcp command-line needed no special handling.
//!
//! ## 🖥 Server mode
//!
//! My test hardware was a 10 year old iMac running OSX 10.15.7 (Darwin kernel 19.6.0).
//!
//! There are two things you need to do to set up qcp so you can connect to it from another machine:
//!
//! #### 1. Set up qcp somewhere the ssh daemon can reach it
//!
//! You need to put the qcp executable somewhere that the ssh server can find it on a non-interactive login.
//! By default OSX's sshd is quite tightly locked down, and the introduction of System Integrity Protection
//! in 10.15 (Catalina) means not even root can write to `/usr/bin`.
//!
//! There are a few ways to achieve this. (Maybe some kind OSX code signing expert can help me figure out
//! whether code signing will let me install qcp directly into /usr/bin/ ?)
//!
//! ##### (a) Set up qcp as an ssh subsystem
//!
//! 1. Decide where you're going to put qcp. I chose to put it in `/usr/local/bin`;
//!    adjust these instructions to suit.
//!
//! 2. Add this to `/etc/sshd/sshd_config` :
//!
//!     `Subsystem qcp /usr/local/bin/qcp --server`
//!
//! 3. Restart sshd. (Same as in step b.4 above.)
//!
//! 4. Configure the client to use subsystem mode
//!
//!    Use the `--ssh-subsystem` argument on the CLI, or put `SshSubsystem 1` in your config file.
//!
//! ##### (b) Configure sshd path
//!
//! 1. Decide where you're going to put qcp. I chose to put it in `/usr/local/bin`;
//!    adjust these instructions to suit.
//!
//! 2. Set your PATH in your per-user ssh environment.
//!    Put this into `~/.ssh/environment` :
//!
//!     `PATH=$PATH:/usr/local/bin`
//!
//!     (note: putting this into your bashrc or zshrc probably won't work as the ssh server is locked down).
//!
//! 3. Allow user-specified paths on ssh connections. Set this in `/etc/ssh/sshd_config` :
//!
//!     `PermitUserEnvironment PATH`
//!
//!     _(note that this is a reduction in security, but not as scary as disabling SIP)_
//! 4. Restart sshd. This did the trick for me:
//!
//!     `sudo launchctl kickstart -k system/com.openssh.sshd`
//!
//! ##### (c) ~Disable SIP~
//!
//! <div class="warning">
//! I really do not recommend this course of action, so I'm not going to explain how to do it.
//! It's quite a security risk and I mention it only for completeness.
//! </div>
//!
//! That said, if you have already decided you want to live on the edge by disabling SIP,
//! then copying the qcp binary into `/usr/bin` is the easiest way to get it going.
//!
//! #### 2. Allow access through the OSX firewall
//!
//! You need to configure the OSX firewall to allow incoming connections to qcp.
//! If you don't, the machine initiating the connection will complain of a protocol
//! timeout setting up the QUIC session.
//!
//! There are a couple of options here too:
//!
//! ##### (a) Make an inbound qcp connection to your OSX machine
//! You should get an OSX dialog asking "Do you want the application “qcp” to accept incoming network connections?"
//! Say yes, obviously.
//!
//! ##### (b) Manual firewall configuration
//!
//! Manually add qcp as an application allowed to receive incoming traffic.
//!
//! On 10.15.7 you can set this up via System Settings > Security & Privacy > Firewall.
//!
//! On newer OSX I understand this may be under System Settings > Network > Firewall.
//!
//! ## 🚀 Network tuning
//!
//! I found the kernel imposes an 8MB UDP send/receive buffer limit by default.
//! qcp performed well straight out of the box; no sysctl configuration was necessary.
//! Nevertheless, we still check the buffer sizes at runtime, in case the kernel is
//! configured differently.
//!
//! On the hardware I have to hand (a 10 year old iMac) I was able to
//! nearly-saturate my 300Mbit fibre downlink pulling from a server with
//! ~310ms ping time. This is the same result as on my Linux desktop.
//!
//! ## 🕵️ Troubleshooting
//!
//! #### Command not found (connecting to an OSX machine)
//! The control channel fails with a message like one of these?
//!
//! ```text
//! bash: line 1: qcp: command not found
//!
//! zsh:1: command not found: qcp
//! ```
//!
//! qcp was not on the path for the remote user. See [set up qcp somewhere the ssh daemon can reach it](#1-set-up-qcp-somewhere-the-ssh-daemon-can-reach-it).
//!
//! ### Protocol timeout setting up the QUIC session
//!
//! Most likely the OSX firewall, or some other intervening firewall, is blocking the
//! UDP packets.
//!
//! Check the OSX firewall first. [See above](#2-allow-access-through-the-osx-firewall).
