//! Options specific to qcp client-mode
// (c) 2024 Ross Younger

use super::{CopyJobSpec, FileSpec};
use clap::Parser;

#[derive(Debug, Parser, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
/// Client-side options which may be provided on the command line, but are not persistent configuration options.
pub struct Parameters {
    /// Enable detailed debug output
    ///
    /// This has the same effect as setting `RUST_LOG=qcp=debug` in the environment.
    /// If present, `RUST_LOG` overrides this option.
    #[arg(short, long, action, help_heading("Debug"), display_order(0))]
    pub debug: bool,

    /// Log to a file
    ///
    /// By default the log receives everything printed to stderr.
    /// To override this behaviour, set the environment variable `RUST_LOG_FILE_DETAIL` (same semantics as `RUST_LOG`).
    #[arg(
        short('l'),
        long,
        action,
        value_name("FILE"),
        help_heading("Output"),
        next_line_help(true),
        display_order(0)
    )]
    pub log_file: Option<String>,

    /// Quiet mode
    ///
    /// Switches off progress display and statistics; reports only errors
    #[arg(short, long, action, conflicts_with("debug"), help_heading("Output"))]
    pub quiet: bool,

    /// Show additional transfer statistics
    #[arg(
        short = 's',
        long,
        alias("stats"),
        action,
        conflicts_with("quiet"),
        help_heading("Output"),
        display_order(0)
    )]
    pub statistics: bool,

    /// Enables detailed debug output from the remote endpoint
    /// (this may interfere with transfer speeds)
    #[arg(long, action, help_heading("Debug"), display_order(0))]
    pub remote_debug: bool,

    /// Output timing profile data after completion
    #[arg(long, action, help_heading("Output"), display_order(0))]
    pub profile: bool,

    // JOB SPECIFICAION ====================================================================
    // (POSITIONAL ARGUMENTS!)
    /// The source file. This may be a local filename, or remote specified as HOST:FILE or USER@HOST:FILE.
    #[arg(value_name = "SOURCE")]
    pub source: Option<FileSpec>,

    /// Destination. This may be a file or directory. It may be local or remote.
    ///
    /// If remote, specify as HOST:DESTINATION or USER@HOST:DESTINATION; or simply HOST: or USER@HOST: to copy to your home directory there.
    #[arg(value_name = "DESTINATION")]
    pub destination: Option<FileSpec>,
}

impl TryFrom<&Parameters> for CopyJobSpec {
    type Error = anyhow::Error;

    fn try_from(args: &Parameters) -> Result<Self, Self::Error> {
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

impl Parameters {
    /// A best-effort attempt to extract a single remote host string from the parameters.
    ///
    /// # Output
    /// If neither source nor dest are present, `Ok("")`
    /// If at most one of source and dest contains a remote host, `Ok(<host>)`
    ///
    /// # Errors
    /// If both source and dest contain a remote host, Err("Only one remote file argument is supported")
    pub(crate) fn remote_user_host_lossy(&self) -> anyhow::Result<Option<String>> {
        let src_host = self.source.as_ref().and_then(|fs| fs.host.as_ref());
        let dst_host = self.destination.as_ref().and_then(|fs| fs.host.as_ref());
        Ok(if let Some(src_host) = src_host {
            if dst_host.is_some() {
                anyhow::bail!("Only one remote file argument is supported");
            }
            Some(src_host.to_string())
        } else {
            // Destination without source would be an exotic situation, but do our best anyway:
            dst_host.map(std::string::ToString::to_string)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_debug_option() {
        let params = Parameters::parse_from(["test", "--debug"]);
        assert!(params.debug);
    }

    #[test]
    fn test_log_file_option() {
        let params = Parameters::parse_from(["test", "--log-file", "log.txt"]);
        assert_eq!(params.log_file, Some("log.txt".to_string()));
    }

    #[test]
    fn test_quiet_option() {
        let params = Parameters::parse_from(["test", "--quiet"]);
        assert!(params.quiet);
    }

    #[test]
    fn test_statistics_option() {
        let params = Parameters::parse_from(["test", "--statistics"]);
        assert!(params.statistics);
    }

    #[test]
    fn test_remote_debug_option() {
        let params = Parameters::parse_from(["test", "--remote-debug"]);
        assert!(params.remote_debug);
    }

    #[test]
    fn test_profile_option() {
        let params = Parameters::parse_from(["test", "--profile"]);
        assert!(params.profile);
    }

    #[test]
    fn test_source_and_destination() {
        let params = Parameters::parse_from(["test", "source.txt", "destination.txt"]);
        assert_eq!(params.source.unwrap().to_string(), "source.txt");
        assert_eq!(params.destination.unwrap().to_string(), "destination.txt");
    }

    #[test]
    fn test_remote_host_lossy() {
        let params = Parameters::parse_from(["test", "user@host:source.txt", "destination.txt"]);
        assert_eq!(
            params.remote_user_host_lossy().unwrap(),
            Some("user@host".to_string())
        );

        let params = Parameters::parse_from(["test", "source.txt", "user@host:destination.txt"]);
        assert_eq!(
            params.remote_user_host_lossy().unwrap(),
            Some("user@host".to_string())
        );

        let params = Parameters::parse_from(["test", "source.txt", "destination.txt"]);
        assert_eq!(params.remote_user_host_lossy().unwrap(), None);
    }

    #[test]
    fn test_copy_job_spec_conversion() {
        let params = Parameters::parse_from(["test", "host:source.txt", "destination.txt"]);
        let copy_job_spec = CopyJobSpec::try_from(&params).unwrap();
        assert_eq!(copy_job_spec.source.to_string(), "host:source.txt");
        assert_eq!(copy_job_spec.destination.to_string(), "destination.txt");
    }
}
