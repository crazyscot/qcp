//! qcp utility - main entrypoint
// (c) 2024 Ross Younger

use qcp::styles::{ERROR, RESET};
use std::process::ExitCode;

use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() -> ExitCode {
    if qcp::cli().unwrap_or_else(|e| {
        if qcp::util::tracing_is_initialised() {
            tracing::error!("{e}");
        } else {
            eprintln!("{ERROR}Error:{RESET} {e}");
        }
        false
    }) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
