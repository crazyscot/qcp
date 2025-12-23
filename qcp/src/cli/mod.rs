//! Command Line Interface for qcp
// (c) 2024 Ross Younger
mod args;
pub(crate) use args::CliArgs;
mod cli_main;
pub mod styles;
pub use cli_main::cli;
mod manpage;
