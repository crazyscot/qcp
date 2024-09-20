// QCP general utility code that didn't fit anywhere else
// (c) 2024 Ross Younger

use std::net::IpAddr;

use anyhow::Context as _;

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
    let format = fmt::layer().compact().with_writer(std::io::stderr);
    tracing_subscriber::registry()
        .with(format)
        .with(filter)
        .init();
    Ok(())
}

// I am a little surprised that this enum, or something similar, doesn't appear in std::net.
#[derive(Debug)]
pub enum AddressFamily {
    Any,
    IPv4,
    IPv6,
}

/// DNS lookup helper
/// Results can be restricted to a given address family.
/// Only the first matching result is returned.
/// If there are no matching records of the required type, returns an error.
pub fn lookup_host_by_family(host: &str, desired: AddressFamily) -> anyhow::Result<IpAddr> {
    let candidates = dns_lookup::lookup_host(host)
        .with_context(|| format!("host name lookup for {host} failed"))?;
    let mut it = candidates.iter();

    let found = match desired {
        AddressFamily::Any => it.next(),
        AddressFamily::IPv4 => it.find(|addr| addr.is_ipv4()),
        AddressFamily::IPv6 => it.find(|addr| addr.is_ipv6()),
    };
    found
        .map(|i| i.to_owned())
        .ok_or(anyhow::anyhow!("host {host} found, but not as {desired:?}"))
}
