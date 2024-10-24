//! Job specifications for the client
// (c) 2024 Ross Younger

use std::str::FromStr;

use crate::transport::ThroughputMode;

use super::args::Options;

/// A file source or destination specified by the user
#[derive(Debug, Clone, Default)]
pub struct FileSpec {
    /// The remote host for the file.
    ///
    /// If not present, this is a local file.
    pub host: Option<String>,
    /// Filename
    ///
    /// If this is a destination, it might be a directory.
    pub filename: String,
}

impl FromStr for FileSpec {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.starts_with('[') {
            // Assume raw IPv6 address [1:2:3::4]:File
            match s.split_once("]:") {
                Some((hostish, filename)) => Ok(Self {
                    // lose the leading bracket as well so it can be looked up as if a hostname
                    host: Some(hostish[1..].to_owned()),
                    filename: filename.into(),
                }),
                None => Ok(Self {
                    host: None,
                    filename: s.to_owned(),
                }),
            }
        } else {
            // Host:File or raw IPv4 address 1.2.3.4:File; or just a filename
            match s.split_once(':') {
                Some((host, filename)) => Ok(Self {
                    host: Some(host.to_string()),
                    filename: filename.to_string(),
                }),
                None => Ok(Self {
                    host: None,
                    filename: s.to_owned(),
                }),
            }
        }
    }
}

/// Details of a file copy job.
/// (This is a helper struct for the contents of `CliArgs` .)
#[derive(Debug, Clone)]
pub struct CopyJobSpec {
    pub(crate) source: FileSpec,
    pub(crate) destination: FileSpec,
}

impl CopyJobSpec {
    /// What direction of data flow should we optimise for?
    pub(crate) fn throughput_mode(&self) -> ThroughputMode {
        if self.source.host.is_some() {
            ThroughputMode::Rx
        } else {
            ThroughputMode::Tx
        }
    }
}

impl TryFrom<&Options> for CopyJobSpec {
    type Error = anyhow::Error;

    fn try_from(args: &Options) -> Result<Self, Self::Error> {
        let source = args
            .source
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("source and destination are required"))?
            .clone();
        let destination = args
            .destination
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("source and destination are required"))?
            .clone();

        if !(source.host.is_none() ^ destination.host.is_none()) {
            anyhow::bail!("One file argument must be remote");
        }

        Ok(Self {
            source,
            destination,
        })
    }
}

#[cfg(test)]
mod test {
    type Res = anyhow::Result<()>;
    use human_repr::HumanCount;

    use super::FileSpec;
    use std::str::FromStr;

    #[test]
    fn filename_no_host() -> Res {
        let fs = FileSpec::from_str("/dir/file")?;
        assert!(fs.host.is_none());
        assert_eq!(fs.filename, "/dir/file");
        Ok(())
    }

    #[test]
    fn host_no_file() -> Res {
        let fs = FileSpec::from_str("host:")?;
        assert_eq!(fs.host.unwrap(), "host");
        assert_eq!(fs.filename, "");
        Ok(())
    }

    #[test]
    fn host_and_file() -> Res {
        let fs = FileSpec::from_str("host:file")?;
        assert_eq!(fs.host.unwrap(), "host");
        assert_eq!(fs.filename, "file");
        Ok(())
    }

    #[test]
    fn bare_ipv4() -> Res {
        let fs = FileSpec::from_str("1.2.3.4:file")?;
        assert_eq!(fs.host.unwrap(), "1.2.3.4");
        assert_eq!(fs.filename, "file");
        Ok(())
    }

    #[test]
    fn bare_ipv6() -> Res {
        let fs = FileSpec::from_str("[1:2:3:4::5]:file")?;
        assert_eq!(fs.host.unwrap(), "1:2:3:4::5");
        assert_eq!(fs.filename, "file");
        Ok(())
    }
    #[test]
    fn bare_ipv6_localhost() -> Res {
        let fs = FileSpec::from_str("[::1]:file")?;
        assert_eq!(fs.host.unwrap(), "::1");
        assert_eq!(fs.filename, "file");
        Ok(())
    }
    #[test]
    fn size_is_kb_not_kib() {
        // same mechanism that clap uses
        use humanize_rs::bytes::Bytes;
        let s = "1k".parse::<Bytes>().unwrap();
        assert_eq!(s.size(), 1000);
    }
    #[test]
    fn human_repr_test() {
        assert_eq!(1000.human_count_bare(), "1k");
    }
}
