// DNS helpers
// (c) 2024 Ross Younger

use std::net::IpAddr;

use crate::protocol::control::control_capnp;
use anyhow::Context as _;

// I am a little surprised that this enum, or something similar, doesn't appear in std::net.
#[derive(Debug, Clone, Copy, strum_macros::Display)]
pub enum AddressFamily {
    Any,
    IPv4,
    IPv6,
}

impl From<control_capnp::client_message::ConnectionType> for AddressFamily {
    fn from(value: control_capnp::client_message::ConnectionType) -> Self {
        use control_capnp::client_message::ConnectionType as wire_af;
        match value {
            wire_af::Ipv4 => AddressFamily::IPv4,
            wire_af::Ipv6 => AddressFamily::IPv6,
        }
    }
}

impl From<std::net::IpAddr> for AddressFamily {
    fn from(value: std::net::IpAddr) -> Self {
        match value {
            IpAddr::V4(_) => AddressFamily::IPv4,
            IpAddr::V6(_) => AddressFamily::IPv6,
        }
    }
}

impl TryFrom<AddressFamily> for control_capnp::client_message::ConnectionType {
    type Error = anyhow::Error;

    fn try_from(value: AddressFamily) -> Result<Self, Self::Error> {
        use control_capnp::client_message::ConnectionType as wire_af;
        match value {
            AddressFamily::Any => anyhow::bail!("AddressFamily::Any not supported by protocol"),
            AddressFamily::IPv4 => Ok(wire_af::Ipv4),
            AddressFamily::IPv6 => Ok(wire_af::Ipv6),
        }
    }
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
