//! qcp utility - main entrypoint
// (c) 2024 Ross Younger

use qcp::styles::{ERROR, RESET};
use std::process::ExitCode;

#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

fn main() -> ExitCode {
    if qcp::cli().unwrap_or_else(|e| {
        if qcp::util::tracing_is_initialised() {
            tracing::error!("{e:#}");
        } else {
            eprintln!("{ERROR}Error:{RESET} {e:#}");
        }
        false
    }) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
