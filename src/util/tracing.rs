//! Tracing helpers
// (c) 2024 Ross Younger

use std::{
    fs::File,
    io::Write,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use indicatif::MultiProgress;
use tracing_subscriber::{fmt, prelude::*, EnvFilter, Layer};

const STANDARD_ENV_VAR: &str = "RUST_LOG";
const LOG_FILE_DETAIL_ENV_VAR: &str = "RUST_LOG_FILE_DETAIL";

/// Result type for `filter_for()`
struct FilterResult {
    filter: EnvFilter,
    used_env: bool, // Did we use the environment variable we were requested to?
}

/// Log filter setup:
/// Use a given environment variable; if it wasn't present, log only qcp items at a given trace level.
fn filter_for(trace_level: &str, key: &str) -> anyhow::Result<FilterResult> {
    EnvFilter::try_from_env(key)
        .map(|filter| FilterResult {
            filter,
            used_env: true,
        })
        .or_else(|e| {
            // The env var was unset or invalid. Which is it?
            if std::env::var(key).is_ok() {
                anyhow::bail!("{key} (set in environment) was invalid: {e}");
            }
            // It was unset. Fall back.
            Ok(FilterResult {
                filter: EnvFilter::new(format!("qcp={trace_level}")),
                used_env: false,
            })
        })
}

/// Set up rust tracing, to console (via an optional `MultiProgress`) and optionally to file.
///
/// By default we log only our events (qcp), at a given trace level.
/// This can be overridden by setting `RUST_LOG`.
///
/// For examples, see <https://docs.rs/tracing-subscriber/0.3.18/tracing_subscriber/fmt/index.html#filtering-events-with-environment-variables>
///
/// **CAUTION:** If this function fails, tracing won't be set up; callers must take extra care to report the error.
pub fn setup(
    trace_level: &str,
    display: Option<&MultiProgress>,
    filename: &Option<String>,
) -> anyhow::Result<()> {
    let mut layers = Vec::new();

    /////// Console output, via the MultiProgress if there is one

    let filter = filter_for(trace_level, STANDARD_ENV_VAR)?;
    // If we used the environment variable, show log targets; if we did not, we're only logging qcp, so do not show targets.
    let format = fmt::layer().compact().with_target(filter.used_env);

    match display {
        None => {
            let format = format
                .with_writer(std::io::stderr)
                .with_filter(filter.filter)
                .boxed();
            layers.push(format);
        }
        Some(mp) => {
            let format = format
                .with_writer(ProgressWriter::wrap(mp))
                .with_filter(filter.filter)
                .boxed();
            layers.push(format);
        }
    };

    //////// File output

    if let Some(filename) = filename {
        let out_file = Arc::new(File::create(filename).context("Failed to open log file")?);
        let filter = if std::env::var(LOG_FILE_DETAIL_ENV_VAR).is_ok() {
            FilterResult {
                filter: EnvFilter::try_from_env(LOG_FILE_DETAIL_ENV_VAR)?,
                used_env: true,
            }
        } else {
            filter_for(trace_level, STANDARD_ENV_VAR)?
        };
        let layer = tracing_subscriber::fmt::layer()
            .with_writer(out_file)
            // Same logic for if we used the environment variable.
            .with_target(filter.used_env)
            .compact()
            .with_ansi(false)
            .with_filter(filter.filter)
            .boxed();
        layers.push(layer);
    }

    ////////

    tracing_subscriber::registry().with(layers).init();

    Ok(())
}

/// A wrapper type so tracing can output in a way that doesn't mess up `MultiProgress`
struct ProgressWriter {
    display: MultiProgress,
}

impl ProgressWriter {
    fn wrap(display: &MultiProgress) -> Mutex<Self> {
        Mutex::new(Self {
            display: display.clone(),
        })
    }
}

impl Write for ProgressWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let msg = std::str::from_utf8(buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if self.display.is_hidden() {
            eprintln!("{msg}");
        } else {
            self.display.println(msg)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn setup_tracing_for_tests() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::DEBUG)
        .init();
}
