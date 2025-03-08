// (c) 2024 Ross Younger

//! # 🚀 Performance tuning
//!
//! There's probably a whole book to be written about this.
//!
//! I've spent some time tuning this for my use case and leave some hooks so you can experiment.
//!
//! **It is critical to understand that the Internet is a strange place with many variables, which will likely confound any experiment you may try.**
//!
//! In my experience, long-distance traffic flows vary wildly from second to second.
//! This is why I added a near-instant (last 1s) bandwidth readout, as well the average.
//!
//! I found that the throughput from my build server (data flow from Europe to NZ) is sometimes very
//! fast, able to saturate my 300Mbit last-mile downlink, and sometimes falls back to hundreds of
//! kilobits or even worse. But the way QUIC applies congestion control worked around this really well.
//! Throughput accelerates rapidly when congestion clears; I _think_ this is subjectively much faster than scp does, but I've not yet gathered the data to do a proper statistical analysis.
//!
//! Yes, it's inefficient to do an ssh handshake and then a QUIC/TLS handshake.
//! But the cost of doing so isn't much in absolute terms (sometimes a few seconds),
//! and this amortises nicely over a large file transfer.
//!
//! ### FAQs
//!
//! #### When sending data to a remote machine, why does performance start fast, stop and then resume?
//! This is an illusion. The meters show the rate at which data is being accepted by the QUIC layer, which is
//! not necessarily the rate at which it is actually going onto the network.
//!
//! When you initiate a file transfer, the initial congestion window is by default quite low.
//! The qcp client reads a load of data from disk and passes it on to QUIC, which causes the meter to go high,
//! but QUIC is only trickling it out onto the network.
//! The QUIC send buffer quickly fills up and it stops accepting any more from qcp so performance
//! appears to drop, sometimes to zero.
//! Before long, QUIC has sent enough that it can accept more from qcp.
//! It doesn't take long for the congestion algorithm to work and data transfer to accelerate.
//!
//! You can demonstrate this, in quiet network conditions, by setting the `--initial-congestion-window` option
//! to something large like a megabyte.
//!
//! ### Tips
//!
//! * When qcp tells you to set up the kernel UDP buffers, do so; they really make a difference. **You need to do this on both machines.**
//! * Run `qcp -h` and study the network tuning options available to you.
//!   * With bandwidth and RTT - at least on my network conditions - I've found that perfect accuracy of configuration isn't so important, as long as it's in the right ballpark.
//!     * In many cases your _last-mile_ bandwidth, i.e. whatever you're paying your ISP for,
//!       is a good setting to use.
//!       But there's a trap here: ISPs often describe their packages in bits per second,
//!       but qcp expects a configuration in bytes!
//!   * Try out `--congestion bbr` if you like. Sometimes it helps.
//!     But understand that it remains experimental, and it does send out a lot more packets.
//!     _If you have a metered connection, this may be an issue!_
//!     * "[When to use and not use BBR](https://blog.apnic.net/2020/01/10/when-to-use-and-not-use-bbr/)"
//!     * [BBR FAQ](https://github.com/google/bbr/blob/master/Documentation/bbr-faq.md)
//!   * Play with the initial congestion window if you like. Sometimes it helps, but it is very situation specific.
//!     I have generally found it helps when the end-to-end network is quiet and there is little congestion,
//!     but it hinders when the network is busy.
//! * Watch out for either end becoming CPU bound. One of my test machines on my local LAN was unable to move more than 7MB/s. It turned out that its CPU was so old it didn't have on-silicon AES. If that applies to you, unfortunately you're not going to be able to move data any faster without a hardware upgrade.
//!   (But [#14](https://github.com/crazyscot/qcp/issues/14) might help a bit.)
//! * If you want to copy multiple files to/from the same remote machine, ssh connection multiplexing will save you a few seconds for each. (You can visualise the difference with the `--profile` option.)
//! * The `--debug` option will report additional information that might help you diagnose configuration issues.
//!
//! ### Reporting
//!
//! qcp will report the number of congestion events it detected, unless you run in `-q` mode.
//!
//! You might find it useful to run in `--stats` mode for additional insights; here's the output from a typical run:
//!
//! ```log
//! 2024-10-14T09:20:52.543540Z  INFO Transferred 104.9MB in 12.75s; average 8.2MB/s
//! 2024-10-14T09:20:52.543782Z  INFO Total packets sent: 3,279 by us; 75,861 by remote
//! 2024-10-14T09:20:52.543955Z  WARN Congestion events detected: 2
//! 2024-10-14T09:20:52.544138Z  WARN Remote lost packets: 112/75.9k (0.15%, for 157kB)
//! 2024-10-14T09:20:52.544320Z  INFO Path MTU 1452, round-trip time 303.1ms, final congestion window 15,537,114
//! 2024-10-14T09:20:52.544530Z  INFO 3.3k datagrams sent, 75.7k received, 0 black holes detected
//! 2024-10-14T09:20:52.544715Z  INFO 107,526,015 total bytes sent for 104,857,600 bytes payload  (2.54% overhead/loss)
//! 2024-10-14T09:20:52.544903Z  WARN Measured path RTT 303.128843ms was greater than configuration 300; for better performance, next time try --rtt 304
//! ```
//!
//! (This was with a 100MB test file, which isn't always enough for the protocol to get fully up to speed.)
