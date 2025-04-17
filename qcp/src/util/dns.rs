//! DNS helpers
// (c) 2024 Ross Younger

use std::net::IpAddr;

use anyhow::Context as _;

use super::AddressFamily;

/// DNS lookup helper
///
/// Results can be restricted to a given address family.
/// Only the first matching result is returned.
/// If there are no matching records of the required type, returns an error.
pub(crate) fn lookup_host_by_family(host: &str, desired: AddressFamily) -> anyhow::Result<IpAddr> {
    let candidates = dns_lookup::lookup_host(host)
        .with_context(|| format!("host name lookup for {host} failed"))?;
    let mut it = candidates.iter();

    let found = match desired {
        AddressFamily::Any => it.next(),
        AddressFamily::Inet => it.find(|addr| addr.is_ipv4()),
        AddressFamily::Inet6 => it.find(|addr| addr.is_ipv6()),
    };
    found
        .map(std::borrow::ToOwned::to_owned)
        .ok_or(anyhow::anyhow!("host {host} found, but not as {desired:?}"))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::AddressFamily;
    use super::lookup_host_by_family;

    #[test]
    fn ipv4() {
        let result = lookup_host_by_family("dns.google", AddressFamily::Inet).unwrap();
        assert!(result.is_ipv4());
    }
    #[test]
    fn ipv6() {
        let result = lookup_host_by_family("dns.google", AddressFamily::Inet6).unwrap();
        assert!(result.is_ipv6());
    }

    #[test]
    fn failure() {
        let result = lookup_host_by_family("no.such.host.invalid", AddressFamily::Any);
        assert!(result.is_err());
    }
}
