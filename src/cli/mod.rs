/// Command Line Interface for qcp
/// (c) 2024 Ross Younger
mod args;
mod cli_main;
mod styles;
pub(crate) use args::{CliArgs, MODE_OPTIONS};
pub use cli_main::cli;
