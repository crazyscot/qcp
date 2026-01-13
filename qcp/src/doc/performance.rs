// (c) 2024 Ross Younger

//! # 🚀 Performance tuning
//!
//! There's probably a whole book to be written about this.
//!
//! I've spent some time tuning this for my use case and have left some hooks so you can experiment.
//!
//! **It is critical to understand that the Internet is a complex system with many variables, which will likely confound any experiment you may try.**
//!
//! In my experience, long-distance traffic flows vary wildly from second to second.
//! This is why I added a near-instant (last 1s) bandwidth readout, as well the average.
//!
//! I found that the throughput from my build server (data flow from Europe to NZ) is sometimes very
//! fast, able to saturate a 300Mbit last-mile downlink, and sometimes falls back to hundreds of
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
//! #### How does cryptography affect performance?
//!
//! If a CPU does not have AES support, you get better performance with the `TLS13_CHACHA20_POLY1305_SHA256` cipher suite.
//!
//! Since v0.6, qcp attempts to autodetect the CPU's capabilities and select a cipher suite ordering appropriately.
//! If you have a requirement to use AES256 as far as possible, you can override this with the `aes256` configuration.
//!
//! The following performance data was gathered using qcp on a 1Gbit LAN connection with <1ms ping time:
//!
//! | Client hardware | Server hardware | AES128 throughput | ChaCha20 throughput |
//! | --------------- | --------------- | ----------------- | ------------------- |
//! | Raspberry Pi 4 (4 cores, 1.5GHz; no AES support)   | Intel Ultra7 265K | 10MB/s | 13MB/s |
//! | Intel Atom D510 (4 cores, 1.66GHz; no AES support) | Intel Ultra7 265K | 10MB/s | 20MB/s |
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
//! * Watch out for either end becoming CPU bound. One of my test machines on my local LAN was unable to move more than 7MB/s. It turned out that its CPU was so old it didn't have on-silicon AES.
//!   If that applies to you, [#14](https://github.com/crazyscot/qcp/issues/14) might help a bit,
//!   but unfortunately you're not going to be able to move data any faster without a hardware upgrade.
//! * If you want to make multiple QCP connections to the same remote machine, ssh connection multiplexing will save you a few seconds for each.
//!   (You can visualise the difference with the `--profile` option.)
//!   Since 0.8, prefer multi-file transfer mode where possible.
//! * The `--debug` option will report additional information that might help you diagnose configuration issues.
//! * If you build qcp yourself, make sure to build in release mode (`cargo build --release --locked`).
//!   On my current desktop PC, debug mode takes about 70% of one core on a 1Gbit LAN transfer, but release mode takes only 15%.
//!   My previous desktop PC could only sustain around 40MB/s in debug mode on the same network.
//! * Make sure the drives at either end are up to the speed you want.  Mechanical HDDs are going to have a
//!   hard time keeping up with modern networks, though OS write caching will help (assuming you have enough RAM).
//! * As of v0.8, the new multi-file and recursive transfer modes don't always perform brilliantly with lots of small files.
//!   (With longer RTTs they do out-perform scp, but not by much.) In some cases it will be more efficient to aggregate smaller files into an archive.
//!
//!
//! ### Reporting
//!
//! qcp will report the number of congestion events it detected, unless you run in `-q` mode.
//!
//! You might find it useful to run in `--stats` mode for additional insights; here's the output from a typical run:
//!
//! ```log
//! 2026-01-09 21:29:54L  INFO Transferred 1.07GB in 15.27s; average 70.3MB/s; peak 100.2MB/s
//! 2026-01-09 21:29:54L  INFO Total packets sent: 24,569 by us; 778,416 by remote
//! 2026-01-09 21:29:54L  WARN Congestion events detected: 24
//! 2026-01-09 21:29:54L  WARN Remote lost packets: 7.6k/778.4k (0.97%, for 10.8MB)
//! 2026-01-09 21:29:54L  INFO Path MTU 1426 (remote: 1426), round-trip time 268.3ms (remote: 269.9ms), final congestion window 28,322,844
//! 2026-01-09 21:29:54L  INFO 24.6k datagrams sent, 770.9k received, 0 black holes detected
//! 2026-01-09 21:29:54L  INFO 1,109,983,211 total bytes sent for 1,073,741,824 bytes payload  (3.38% overhead/loss)
//! ```
