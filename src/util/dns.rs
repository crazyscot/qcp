// DNS helpers
// (c) 2024 Ross Younger

use std::net::IpAddr;

use crate::protocol::control::ConnectionType;
use anyhow::Context as _;

/// DNS lookup helper
/// Results can be restricted to a given address family.
/// Only the first matching result is returned.
/// If there are no matching records of the required type, returns an error.
pub fn lookup_host_by_family(
    host: &str,
    desired: Option<ConnectionType>,
) -> anyhow::Result<IpAddr> {
    let candidates = dns_lookup::lookup_host(host)
        .with_context(|| format!("host name lookup for {host} failed"))?;
    let mut it = candidates.iter();

    let found = match desired {
        None => it.next(),
        Some(ConnectionType::Ipv4) => it.find(|addr| addr.is_ipv4()),
        Some(ConnectionType::Ipv6) => it.find(|addr| addr.is_ipv6()),
    };
    found
        .map(std::borrow::ToOwned::to_owned)
        .ok_or(anyhow::anyhow!("host {host} found, but not as {desired:?}"))
}
