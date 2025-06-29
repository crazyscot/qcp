//! Tracing helpers
// (c) 2024 Ross Younger

use std::{
    fs::File,
    io::Write,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
};

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

use crate::cli::styles::maybe_strip_color;

static TRACING_INITIALIZED: AtomicBool = AtomicBool::new(false);

const FRIENDLY_FORMAT_LOCAL: &str = "%Y-%m-%d %H:%M:%SL";
const FRIENDLY_FORMAT_UTC: &str = "%Y-%m-%d %H:%M:%SZ";

/// Environment variable that controls what gets logged to stderr
const STANDARD_ENV_VAR: &str = "RUST_LOG";
/// Environment variable that controls what gets logged to file
const LOG_FILE_DETAIL_ENV_VAR: &str = "RUST_LOG_FILE_DETAIL";

/// Computes the trace level for a given set of [crate::client::Parameters]
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
                filter: EnvFilter::try_new(format!("qcp={trace_level}"))?,
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

pub(crate) enum ConsoleTraceType {
    /// Trace directly to console
    Standard,
    /// Trace via Indicatif. Note that a [`SetupFn`] will consume this enum, so you may need to clone the [`MultiProgress`] (which is cheap, it's a reference type really)
    Indicatif(MultiProgress),
    /// Do not print traces anywhere
    #[allow(dead_code)] // this is used by tests
    None,
}

/// Function type for [`setup`]
pub(crate) type SetupFn =
    fn(&str, ConsoleTraceType, Option<&String>, TimeFormat, bool) -> anyhow::Result<()>;

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
pub(crate) fn setup(
    trace_level: &str,
    display: ConsoleTraceType,
    log_file: Option<&String>,
    time_format: TimeFormat,
    ansi_colours: bool,
) -> anyhow::Result<()> {
    if is_initialized() {
        tracing::warn!("tracing::setup called a second time (ignoring)");
        return Ok(());
    }
    TRACING_INITIALIZED.store(true, Ordering::Relaxed);

    let layers = setup_inner(trace_level, display, log_file, time_format, ansi_colours)?;
    tracing_subscriber::registry().with(layers).init();

    Ok(())
}

pub(crate) fn setup_inner(
    trace_level: &str,
    display: ConsoleTraceType,
    log_file: Option<&String>,
    time_format: TimeFormat,
    ansi_colours: bool,
) -> anyhow::Result<
    Vec<Box<dyn tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync>>,
> {
    let mut layers = Vec::new();

    /////// Console output, via the MultiProgress if there is one

    let filter = filter_for(trace_level, STANDARD_ENV_VAR)?;
    // If we used the environment variable, show log targets; if we did not, we're only logging qcp, so do not show targets.

    match display {
        ConsoleTraceType::None => (),
        ConsoleTraceType::Standard => {
            layers.push(make_tracing_layer(
                std::io::stderr,
                filter.filter,
                time_format,
                filter.used_env,
                ansi_colours,
            ));
        }
        ConsoleTraceType::Indicatif(mp) => {
            layers.push(make_tracing_layer(
                ProgressWriter::wrap(mp),
                filter.filter,
                time_format,
                filter.used_env,
                ansi_colours,
            ));
        }
    }

    //////// File output

    if let Some(filename) = log_file {
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

    Ok(layers)
}

/// Returns whether tracing has been initialized
pub(crate) fn is_initialized() -> bool {
    TRACING_INITIALIZED.load(Ordering::Relaxed)
}

/// A wrapper type so tracing can output in a way that doesn't mess up `MultiProgress`
struct ProgressWriter(MultiProgress);

impl ProgressWriter {
    fn wrap(display: MultiProgress) -> Mutex<Self> {
        Mutex::new(Self(display))
    }
}

impl Write for ProgressWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let msg = std::str::from_utf8(buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let msg = maybe_strip_color(msg);
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
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use indicatif::{MultiProgress, ProgressDrawTarget};
    use pretty_assertions::assert_eq;
    use rusty_fork::rusty_fork_test;
    use tracing_subscriber::EnvFilter;

    use super::{setup, setup_inner};
    use crate::{
        Parameters,
        util::{TimeFormat, tracing::ConsoleTraceType},
    };

    use littertray::LitterTray;

    #[test]
    fn trace_levels() {
        use super::trace_level;
        let p = Parameters {
            debug: true,
            quiet: true,
            ..Default::default()
        };
        assert_eq!(trace_level(&p), "debug");
        let p = Parameters {
            quiet: true,
            ..Default::default()
        };
        assert_eq!(trace_level(&p), "error");
        let p = Parameters::default();
        assert_eq!(trace_level(&p), "info");
    }

    #[test]
    fn test_create_layers_with_console_output() {
        let mp = MultiProgress::new();
        let layers = setup_inner(
            "info",
            ConsoleTraceType::Indicatif(mp),
            None,
            TimeFormat::Local,
            false,
        )
        .unwrap();
        assert_eq!(layers.len(), 1); // Only one layer for console output
    }

    #[test]
    fn test_create_layers_with_file_output() {
        LitterTray::run(|_| {
            let filename = String::from("test.log");
            let layers = setup_inner(
                "info",
                ConsoleTraceType::Standard,
                Some(&filename),
                TimeFormat::Utc,
                false,
            )
            .unwrap();
            assert_eq!(layers.len(), 2); // One for console, one for file
        });
    }

    #[test]
    fn test_create_layers_with_invalid_level() {
        let result = setup_inner(
            "invalid_level",
            ConsoleTraceType::None,
            None,
            TimeFormat::Utc,
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_tracing_layer_rfc3339() {
        let f = EnvFilter::new("");
        let _result: Box<
            dyn tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync,
        > = super::make_tracing_layer(std::io::stderr, f, TimeFormat::Rfc3339, false, false);
        // it doesn't seem possible to usefully test the created layer at the moment
    }

    #[test]
    fn test_progress_writer() {
        use std::io::Write as _;
        let mp = MultiProgress::new();
        let mux = super::ProgressWriter::wrap(mp);
        let mut writer = mux.lock().unwrap();
        let msg = "Test message";
        let bytes_written = writer.write(msg.as_bytes()).unwrap();
        assert_eq!(bytes_written, msg.len());
        writer.flush().unwrap();
    }
    #[test]
    fn test_progress_writer_hidden() {
        use std::io::Write as _;
        let mp = MultiProgress::with_draw_target(ProgressDrawTarget::hidden());
        let mux = super::ProgressWriter::wrap(mp);
        let mut writer = mux.lock().unwrap();
        let msg = "Test message";
        let bytes_written = writer.write(msg.as_bytes()).unwrap();
        assert_eq!(bytes_written, msg.len());
        writer.flush().unwrap();
    }

    // these tests affect global state, so need to run in forks
    rusty_fork_test! {
        #[test]
        fn test_setup_initialization() {
            let result1 = setup("info", ConsoleTraceType::None, None, TimeFormat::Utc, false);
            assert!(result1.is_ok());

            let result2 = setup("info", ConsoleTraceType::None, None, TimeFormat::Utc, false);
            assert!(result2.is_ok()); // Second call should succeed but be ignored
        }

        #[test]
        fn setup_tracing() {
            super::setup("debug", ConsoleTraceType::None, None, TimeFormat::Utc, false).unwrap();
            // a second call must succeed (albeit with a warning)
            super::setup("debug", ConsoleTraceType::None, None, TimeFormat::Utc, false).unwrap();
        }
    }
}
