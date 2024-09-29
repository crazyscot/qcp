// qcp server command line interface
// (c) 2024 Ross Younger

use crate::build_info;
use clap::Parser;
use human_units::Size;

#[derive(Clone, Copy, Debug, Parser)]
#[command(
    author,
    version(build_info::GIT_VERSION),
    about,
    long_about = "This is the QUIC file copy remote end. It is intended for unattended use. If you want to copy files, you should probably use qcp."
)]
#[command(styles=crate::styles::get())]
pub struct ServerArgs {
    /// Enable detailed debug output
    #[arg(short, long, action)]
    pub debug: bool,

    /// The maximum network bandwidth we expect to/from the target system.
    /// Along with the initial RTT, this directly affects the buffer sizes used.
    /// This may be specified directly as a number of bytes, or as an SI quantity
    /// e.g. "10M" or "256k". Note that this is described in bytes, not bits;
    /// if (for example) you expect to fill a 1Gbit ethernet connection,
    /// 125M might be a suitable upper limit.
    #[arg(short('b'), long, help_heading("Network tuning"), default_value("12M"), value_name="bytes", value_parser=clap::value_parser!(Size))]
    pub bandwidth: Size,

    /// The expected network Round Trip time to the target system, in milliseconds.
    /// Along with the bandwidth limit, this directly affects the buffer sizes used.
    /// (Buffer size = bandwidth * RTT)
    #[arg(
        short('r'),
        long,
        help_heading("Network tuning"),
        default_value("300"),
        value_name("ms")
    )]
    pub rtt: u16,

    /// (Network wizards only! Setting this too high causes a reduction in throughput.)
    /// The initial value for the sending congestion control window.
    /// qcp uses the CUBIC congestion control algorithm. The window grows by the number of bytes acknowledged each time,
    /// until encountering saturation or congestion.
    #[arg(
        short('w'),
        long,
        help_heading("Network tuning"),
        default_value("14720"),
        value_name = "bytes"
    )]
    pub initial_congestion_window: u64,
}
