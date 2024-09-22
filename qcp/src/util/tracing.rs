// Tracing helpers
// (c) 2024 Ross Younger

/// Set up rust tracing.
/// By default we log only our events (qcp), at a given trace level.
/// This can be overridden at any time by setting RUST_LOG.
/// For examples, see https://docs.rs/tracing-subscriber/0.3.18/tracing_subscriber/fmt/index.html#filtering-events-with-environment-variables
pub fn setup_tracing(trace_level: &str) -> anyhow::Result<()> {
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
    let format = fmt::layer()
        .compact()
        .with_target(false)
        .with_writer(std::io::stderr);
    tracing_subscriber::registry()
        .with(format)
        .with(filter)
        .init();
    Ok(())
}
