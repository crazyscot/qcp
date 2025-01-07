//! qcp utility - main entrypoint
// (c) 2024 Ross Younger

use qcp::styles::{ERROR, RESET};

#[cfg(all(target_env = "musl", target_pointer_width = "64"))]
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

fn main() -> std::process::ExitCode {
    match qcp::cli() {
        Ok(code) => code,
        Err(e) => {
            if qcp::util::tracing_is_initialised() {
                tracing::error!("{e}");
            } else {
                eprintln!("{ERROR}Error:{RESET} {e}");
            }
            std::process::ExitCode::FAILURE
        }
    }
}
