//! qcp utility - main entrypoint
// (c) 2024 Ross Younger

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use std::process::ExitCode;

use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[cfg_attr(coverage_nightly, coverage(off))]
fn main() -> ExitCode {
    qcp::main()
}
