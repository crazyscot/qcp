// qcp utility - main entrypoint
// (c) 2024 Ross Younger

use clap::Parser as _;
use indicatif::MultiProgress;
use qcp::client::ClientArgs;
use std::process::ExitCode;

fn main() -> anyhow::Result<ExitCode> {
    let args = ClientArgs::parse();
    if args.help_socket_bufsize {
        // One day we might make this a function of the remote host.
        let send_window = qcp::transport::SEND_BUFFER_SIZE;
        let recv_window = qcp::transport::receive_window_for(*args.bandwidth, args.rtt) as usize;
        qcp::os::os::print_udp_buffer_size_help_message(recv_window, send_window);
        return Ok(ExitCode::SUCCESS);
    }

    let progress = MultiProgress::new(); // This writes to stderr
    let trace_level = match args.debug {
        true => "trace",
        false => match args.quiet {
            true => "error",
            false => "info",
        },
    };
    qcp::util::setup_tracing(trace_level, Some(&progress))
        .and_then(|_| qcp::client::main(args, progress))
        .inspect_err(|e| tracing::error!("{e}"))
        .or_else(|_| Ok(false))
        .map(|success| match success {
            true => ExitCode::SUCCESS,
            false => ExitCode::FAILURE,
        })
}
