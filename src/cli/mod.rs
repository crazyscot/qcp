/// Command Line Interface for qcp
/// (c) 2024 Ross Younger
mod args;
mod cli_main;
pub(crate) use args::CliArgs;
pub use cli_main::cli;
