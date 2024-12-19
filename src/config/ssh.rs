//! Config file parsing, openssh-style
// (c) 2024 Ross Younger

mod errors;
pub(crate) use errors::SshConfigError;

mod files;
mod includes;
mod lines;
mod matching;
mod values;

pub(crate) use files::Parser;
pub(crate) use values::Setting;

use includes::find_include_files;
use lines::{split_args, Line};
use matching::evaluate_host_match;
use values::ValueProvider;
