//! Job specifications for the client
// (c) 2024 Ross Younger

use std::str::FromStr;

use crate::transport::ThroughputMode;

/// Strips the optional user@ part off a hostname
fn hostname_of(user_at_host: &str) -> &str {
    user_at_host.split_once('@').unwrap_or(("", user_at_host)).1
}

/// A file source or destination specified by the user
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSpec {
    /// The remote `[user@]host` for the file. This may be a hostname or an IP address.
    /// It may also be a _hostname alias_ that matches a Host section in the user's ssh config file.
    /// (In that case, the ssh config file must specify a HostName.)
    ///
    /// If not present, this is a local file.
    pub user_at_host: Option<String>,
    /// Filename
    ///
    /// If this is a destination, it might be a directory.
    pub filename: String,
}

impl FileSpec {
    /// Returns only the hostname part of the file, if any; the username is stripped.
    pub(crate) fn hostname(&self) -> Option<&str> {
        self.user_at_host.as_ref().map(|s| hostname_of(s))
    }
}

impl FromStr for FileSpec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with('[') {
            // Assume raw IPv6 address [1:2:3::4]:File
            match s.split_once("]:") {
                Some((hostish, filename)) => Ok(Self {
                    // lose the leading bracket as well so it can be looked up as if a hostname
                    user_at_host: Some(hostish[1..].to_owned()),
                    filename: filename.into(),
                }),
                None => Ok(Self {
                    user_at_host: None,
                    filename: s.to_owned(),
                }),
            }
        } else {
            // Host:File or raw IPv4 address 1.2.3.4:File; or just a filename
            match s.split_once(':') {
                Some((host, filename)) => Ok(Self {
                    user_at_host: Some(host.to_string()),
                    filename: filename.to_string(),
                }),
                None => Ok(Self {
                    user_at_host: None,
                    filename: s.to_owned(),
                }),
            }
        }
    }
}

impl std::fmt::Display for FileSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(host) = &self.user_at_host {
            write!(f, "{}:{}", host, self.filename)
        } else {
            write!(f, "{}", self.filename)
        }
    }
}

/// Details of a file copy job.
#[derive(Debug, Clone)]
pub struct CopyJobSpec {
    pub(crate) source: FileSpec,
    pub(crate) destination: FileSpec,
    /// The `[user@]host` part of whichever of the source or destination contained one.
    /// (There can be only one.)
    pub(crate) user_at_host: String,
}

impl CopyJobSpec {
    /// standard constructor
    pub(crate) fn try_new(source: FileSpec, destination: FileSpec) -> anyhow::Result<Self> {
        if !(source.user_at_host.is_none() ^ destination.user_at_host.is_none()) {
            anyhow::bail!("One file argument must be remote");
        }
        let user_at_host = source
            .user_at_host
            .clone()
            .unwrap_or_else(|| destination.user_at_host.clone().unwrap_or_default());

        Ok(Self {
            source,
            destination,
            user_at_host,
        })
    }

    /// What direction of data flow should we optimise for?
    pub(crate) fn throughput_mode(&self) -> ThroughputMode {
        if self.source.user_at_host.is_some() {
            ThroughputMode::Rx
        } else {
            ThroughputMode::Tx
        }
    }

    /// The hostname portion of whichever of the arguments contained one.
    pub(crate) fn remote_host(&self) -> &str {
        hostname_of(&self.user_at_host)
    }
}

#[cfg(test)]
mod test {
    type Res = anyhow::Result<()>;
    use engineering_repr::EngineeringQuantity;

    use super::FileSpec;
    use std::str::FromStr;

    #[test]
    fn filename_no_host() -> Res {
        let fs = FileSpec::from_str("/dir/file")?;
        assert!(fs.user_at_host.is_none());
        assert_eq!(fs.filename, "/dir/file");
        Ok(())
    }

    #[test]
    fn host_no_file() -> Res {
        let fs = FileSpec::from_str("host:")?;
        assert_eq!(fs.user_at_host.unwrap(), "host");
        assert_eq!(fs.filename, "");
        Ok(())
    }

    #[test]
    fn host_and_file() -> Res {
        let fs = FileSpec::from_str("host:file")?;
        assert_eq!(fs.user_at_host.unwrap(), "host");
        assert_eq!(fs.filename, "file");
        Ok(())
    }

    #[test]
    fn bare_ipv4() -> Res {
        let fs = FileSpec::from_str("1.2.3.4:file")?;
        assert_eq!(fs.user_at_host.unwrap(), "1.2.3.4");
        assert_eq!(fs.filename, "file");
        Ok(())
    }

    #[test]
    fn bare_ipv6() -> Res {
        let fs = FileSpec::from_str("[1:2:3:4::5]:file")?;
        assert_eq!(fs.user_at_host.unwrap(), "1:2:3:4::5");
        assert_eq!(fs.filename, "file");
        Ok(())
    }
    #[test]
    fn bare_ipv6_localhost() -> Res {
        let fs = FileSpec::from_str("[::1]:file")?;
        assert_eq!(fs.user_at_host.unwrap(), "::1");
        assert_eq!(fs.filename, "file");
        Ok(())
    }
    #[test]
    fn not_really_ipv6() {
        let spec = FileSpec::from_str("[1:2:3:4::5").unwrap();
        assert_eq!(spec.user_at_host, None);
        assert_eq!(spec.filename, "[1:2:3:4::5");
    }

    #[test]
    fn size_is_kb_not_kib() {
        // same mechanism that clap uses
        let q = "1k".parse::<EngineeringQuantity<u64>>().unwrap();
        assert_eq!(u64::from(q), 1000);
    }
}
