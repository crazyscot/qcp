// QCP general utility code that didn't fit anywhere else
// (c) 2024 Ross Younger

use std::{
    fs::Metadata,
    io::ErrorKind,
    net::IpAddr,
    path::{Path, PathBuf},
    str::FromStr as _,
};

use crate::protocol::session::session_capnp::Status;
use anyhow::Context as _;
use futures_util::TryFutureExt as _;

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

/// Opens a local file for reading, returning a filehandle and metadata.
/// Error type is a tuple ready to send as a Status response.
pub async fn open_file_read(
    filename: &str,
) -> anyhow::Result<(tokio::fs::File, Metadata), (Status, Option<String>, tokio::io::Error)> {
    let path = Path::new(&filename);

    let fh: tokio::fs::File = tokio::fs::File::open(path)
        .await
        .map_err(|e| match e.kind() {
            ErrorKind::NotFound => (Status::FileNotFound, Some(e.to_string()), e),
            ErrorKind::PermissionDenied => (Status::IncorrectPermissions, Some(e.to_string()), e),
            ErrorKind::Other => (Status::IoError, Some(e.to_string()), e),
            _ => (
                Status::IoError,
                Some(format!("unhandled error from File::open: {e}")),
                e,
            ),
        })?;

    let meta = fh
        .metadata()
        .map_err(|e| {
            (
                Status::IoError,
                Some(format!("unable to determine file size: {e}")),
                e,
            )
        })
        .await?;

    Ok((fh, meta))
}

/// Opens a local file for writing, from an incoming FileHeader
pub async fn open_file_write(
    path: &str,
    header: &crate::protocol::session::FileHeader,
) -> anyhow::Result<tokio::fs::File> {
    let mut dest_path = PathBuf::from_str(path).unwrap(); // this is marked as infallible
    let dest_meta = tokio::fs::metadata(&dest_path).await;
    if let Ok(meta) = dest_meta {
        // if it's a file, proceed (overwriting)
        if meta.is_dir() {
            dest_path.push(header.filename.clone());
        } else if meta.is_symlink() {
            // TODO: Need to cope with this case; test whether it's a directory?
            let deref = std::fs::read_link(&dest_path)?;
            if std::fs::metadata(deref).is_ok_and(|meta| meta.is_dir()) {
                dest_path.push(header.filename.clone());
            }
            // Else assume the link points to a file, which we will overwrite.
        }
    }

    let file = tokio::fs::File::create(dest_path).await?;
    file.set_len(header.size).await?;
    Ok(file)
}
