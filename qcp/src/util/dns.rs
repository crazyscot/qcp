//! DNS helpers
// (c) 2024 Ross Younger

use std::net::IpAddr;
use std::net::ToSocketAddrs as _;

use anyhow::Context as _;

use super::AddressFamily;

/// DNS lookup helper
///
/// Results can be restricted to a given address family.
/// Only the first matching result is returned.
/// If there are no matching records of the required type, returns an error.
pub(crate) fn lookup_host_by_family<AF>(host: &str, desired: AF) -> anyhow::Result<IpAddr>
where
    AF: Into<AddressFamily>,
{
    let desired = desired.into();
    let mut it = (host, 0)
        .to_socket_addrs()
        .with_context(|| format!("host name lookup for {host} failed"))?;

    let found = match desired {
        AddressFamily::Any => it.next(),
        AddressFamily::Inet => it.find(std::net::SocketAddr::is_ipv4),
        AddressFamily::Inet6 => it.find(std::net::SocketAddr::is_ipv6),
    };

    found
        .map(|a| a.ip())
        .ok_or(anyhow::anyhow!("host {host} found, but not as {desired:?}"))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::AddressFamily;
    use super::lookup_host_by_family;

    #[tokio::test]
    async fn ipv4() {
        let result = lookup_host_by_family("ipv4.google.com", AddressFamily::Inet).unwrap();
        assert!(result.is_ipv4());
    }
    #[cfg_attr(target_os = "macos", ignore)] // GitHub OSX runners seem unable to look up ipv6.google.com?!
    #[cfg_attr(msvc, ignore)] // GitHub Windows runners seem unable to look up ipv6.google.com?!
    #[tokio::test]
    async fn ipv6() {
        let result = lookup_host_by_family("ipv6.google.com", AddressFamily::Inet6).unwrap();
        assert!(result.is_ipv6());
    }

    #[tokio::test]
    async fn failure() {
        let result = lookup_host_by_family("no.such.host.invalid", AddressFamily::Any);
        assert!(result.is_err());
    }
}
