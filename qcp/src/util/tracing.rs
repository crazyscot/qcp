// Tracing helpers
// (c) 2024 Ross Younger

use std::{io::Write, sync::Mutex};

use indicatif::MultiProgress;

/// Set up rust tracing.
/// By default we log only our events (qcp), at a given trace level.
/// This can be overridden at any time by setting RUST_LOG.
/// For examples, see https://docs.rs/tracing-subscriber/0.3.18/tracing_subscriber/fmt/index.html#filtering-events-with-environment-variables
pub fn setup_tracing(trace_level: &str, progress: Option<&MultiProgress>) -> anyhow::Result<()> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let trace_expr = format!("qcp={trace_level}");
    let filter = EnvFilter::try_from_default_env().or_else(|_| {
        // The env var was unset or invalid. Which is it?
        if std::env::var("RUST_LOG").is_ok() {
            anyhow::bail!("RUST_LOG (set in environment) was invalid");
        }
        // It was unset.
        Ok(EnvFilter::new(trace_expr))
    })?;

    let format = fmt::layer().compact().with_target(false);

    match progress {
        None => {
            let format = format.with_writer(std::io::stderr);
            tracing_subscriber::registry()
                .with(format)
                .with(filter)
                .init();
        }
        Some(mp) => {
            let format = format.with_writer(ProgressWriter::wrap(mp));
            tracing_subscriber::registry()
                .with(format)
                .with(filter)
                .init();
        }
    };
    Ok(())
}

/// A wrapper type so tracing can output in a way that doesn't mess up MultiProgress
struct ProgressWriter {
    progress: MultiProgress,
}

impl ProgressWriter {
    fn wrap(progress: &MultiProgress) -> Mutex<Self> {
        Mutex::new(Self {
            progress: progress.clone(),
        })
    }
}

impl Write for ProgressWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let msg = std::str::from_utf8(buf)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        if self.progress.is_hidden() {
            eprintln!("{msg}");
        } else {
            self.progress.println(msg)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
