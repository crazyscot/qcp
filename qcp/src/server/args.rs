// qcp server command line interface
// (c) 2024 Ross Younger

use crate::build_info;
use clap::Parser;

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
    /// The file buffer size to use (default 2MB; tune to your needs).
    /// We use a larger buffer for network operations.
    #[arg(short('b'), long, default_value("2097152"))]
    pub buffer_size: usize,
}

impl ServerArgs {
    /// Buffer size to use for network operations
    pub(crate) fn network_buffer_size(&self) -> usize {
        self.buffer_size * 4
    }

    /// Buffer size to use for file operations
    pub(crate) fn file_buffer_size(&self) -> usize {
        self.buffer_size
    }
}
