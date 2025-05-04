//! Config file parsing, openssh-style
// (c) 2024 Ross Younger

mod errors;
pub(crate) use errors::ConfigFileError;

mod files;
mod includes;
mod lines;
mod matching;
mod values;

pub(crate) use files::Parser;
pub(crate) use values::Setting;

use includes::find_include_files;
use lines::{Line, split_args};
use matching::evaluate_host_match;
use values::ValueProvider;
