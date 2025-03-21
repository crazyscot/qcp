//! Tracing helpers
// (c) 2024 Ross Younger

use std::{
    fs::File,
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
};

use anstream::eprintln;
use anyhow::Context;
use indicatif::MultiProgress;
use serde::{Deserialize, Serialize, de};
use strum::VariantNames as _;
use tracing_subscriber::{
    EnvFilter,
    fmt::{
        MakeWriter,
        time::{ChronoLocal, ChronoUtc},
    },
    prelude::*,
};

static TRACING_INITIALIZED: AtomicBool = AtomicBool::new(false);

const FRIENDLY_FORMAT_LOCAL: &str = "%Y-%m-%d %H:%M:%SL";
const FRIENDLY_FORMAT_UTC: &str = "%Y-%m-%d %H:%M:%SZ";

/// Environment variable that controls what gets logged to stderr
const STANDARD_ENV_VAR: &str = "RUST_LOG";
/// Environment variable that controls what gets logged to file
const LOG_FILE_DETAIL_ENV_VAR: &str = "RUST_LOG_FILE_DETAIL";

/// Computes the trace level for a given set of [ClientParameters]
pub(crate) fn trace_level(args: &crate::client::Parameters) -> &str {
    if args.debug {
        "debug"
    } else if args.quiet {
        "error"
    } else {
        "info"
    }
}

/// Selects the format of time stamps in output messages
#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Eq,
    PartialEq,
    strum::Display,
    strum::EnumString,
    strum::VariantNames,
    clap::ValueEnum,
    Serialize,
)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "kebab-case")]
pub enum TimeFormat {
    /// Local time (as best as we can figure it out), as "year-month-day HH:MM:SS"
    #[default]
    Local,
    /// UTC time, as "year-month-day HH:MM:SS"
    Utc,
    /// UTC time, in the format described in [RFC 3339](https://datatracker.ietf.org/doc/html/rfc3339).
    ///
    /// Examples:
    /// `1997-11-12T09:55:06-06:00`
    /// `2010-03-14T18:32:03Z`
    Rfc3339,
}

impl<'de> Deserialize<'de> for TimeFormat {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let lower = s.to_ascii_lowercase();
        // requires strum::EnumString && strum::VariantNames && #[strum(serialize_all = "lowercase")]
        std::str::FromStr::from_str(&lower)
            .map_err(|_| de::Error::unknown_variant(&s, TimeFormat::VARIANTS))
    }
}

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
                anyhow::bail!("{key} (set in environment) was not understood: {e}");
            }
            // It was unset. Fall back.
            Ok(FilterResult {
                filter: EnvFilter::new(format!("qcp={trace_level}")),
                used_env: false,
            })
        })
}

fn make_tracing_layer<S, W, F>(
    writer: W,
    filter: F,
    time_format: TimeFormat,
    show_target: bool,
    ansi: bool,
) -> Box<dyn tracing_subscriber::Layer<S> + Send + Sync>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
    W: for<'writer> MakeWriter<'writer> + 'static + Sync + Send,
    F: tracing_subscriber::layer::Filter<S> + 'static + Sync + Send,
{
    // The common bit
    let layer = tracing_subscriber::fmt::layer::<S>()
        .compact()
        .with_target(show_target)
        .with_ansi(ansi);

    // Unfortunately, you have to add the timer before you can add the writer and filter, so
    // there's a bit of duplication here:
    match time_format {
        TimeFormat::Local => layer
            .with_timer(ChronoLocal::new(FRIENDLY_FORMAT_LOCAL.into()))
            .with_writer(writer)
            .with_filter(filter)
            .boxed(),
        TimeFormat::Utc => layer
            .with_timer(ChronoUtc::new(FRIENDLY_FORMAT_UTC.into()))
            .with_writer(writer)
            .with_filter(filter)
            .boxed(),

        TimeFormat::Rfc3339 => layer
            .with_timer(ChronoLocal::rfc_3339())
            .with_writer(writer)
            .with_filter(filter)
            .boxed(),
    }
}

/// Set up rust tracing, to console (via an optional `MultiProgress`) and optionally to file.
///
/// By default we log only our events (qcp), at a given trace level.
/// This can be overridden by setting `RUST_LOG`.
///
/// For examples, see <https://docs.rs/tracing-subscriber/0.3.18/tracing_subscriber/fmt/index.html#filtering-events-with-environment-variables>
///
/// **CAUTION:** If this function fails, tracing won't be set up; callers must take extra care to report the error.
///
/// **NOTE:** You can only run this once per process. A global bool prevents re-running.
pub fn setup(
    trace_level: &str,
    display: Option<&MultiProgress>,
    filename: &Option<String>,
    time_format: TimeFormat,
) -> anyhow::Result<()> {
    if is_initialized() {
        tracing::warn!("tracing::setup called a second time (ignoring)");
        return Ok(());
    }
    TRACING_INITIALIZED.store(true, Ordering::Relaxed);

    let mut layers = Vec::new();

    /////// Console output, via the MultiProgress if there is one

    let filter = filter_for(trace_level, STANDARD_ENV_VAR)?;
    // If we used the environment variable, show log targets; if we did not, we're only logging qcp, so do not show targets.

    match display {
        None => {
            layers.push(make_tracing_layer(
                std::io::stderr,
                filter.filter,
                time_format,
                filter.used_env,
                true,
            ));
        }
        Some(mp) => {
            layers.push(make_tracing_layer(
                ProgressWriter::wrap(mp),
                filter.filter,
                time_format,
                filter.used_env,
                true,
            ));
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
        // Same logic for whether we used the environment variable.
        layers.push(make_tracing_layer(
            out_file,
            filter.filter,
            time_format,
            filter.used_env,
            false,
        ));
    }

    ////////

    tracing_subscriber::registry().with(layers).init();

    Ok(())
}

/// Returns whether tracing has been initialized
pub fn is_initialized() -> bool {
    TRACING_INITIALIZED.load(Ordering::Relaxed)
}

/// A wrapper type so tracing can output in a way that doesn't mess up `MultiProgress`
struct ProgressWriter(MultiProgress);

impl ProgressWriter {
    fn wrap(display: &MultiProgress) -> Mutex<Self> {
        Mutex::new(Self(display.clone()))
    }
}

impl Write for ProgressWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let msg = std::str::from_utf8(buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if self.0.is_hidden() {
            eprintln!("{msg}");
        } else {
            self.0.println(msg)?;
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
