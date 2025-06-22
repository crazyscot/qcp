//! This is a pure-testing crate.
//! It consists of tests that live external to the main qcp crate, because they require unsafe Rust.

#![cfg_attr(coverage_nightly, feature(coverage_attribute), coverage(off))]
#![cfg(test)]

mod cli;
mod clicolor;
mod manager;
mod ssh_includes;
mod styles;
mod test_windows;
