//! Options specific to qcp client-mode
// (c) 2024 Ross Younger

use super::{CopyJobSpec, FileSpec};
use crate::config::Source as ConfigSource;
use clap::Parser;

const META_JOBSPEC: &str = "command-line (user@host)";

#[derive(Debug, Parser, Clone, Default)]
#[allow(clippy::struct_excessive_bools)]
/// Client-side options which may be provided on the command line, but are not persistent configuration options.
pub struct Parameters {
    /// Enable detailed debug output
    ///
    /// This has the same effect as setting `RUST_LOG=qcp=debug` in the environment.
    /// If present, `RUST_LOG` overrides this option.
    #[arg(long, help_heading("Debug"), display_order(0))]
    pub debug: bool,

    /// Log to a file
    ///
    /// By default the log receives everything printed to stderr.
    /// To override this behaviour, set the environment variable `RUST_LOG_FILE_DETAIL` (same semantics as `RUST_LOG`).
    #[arg(
        long,
        value_name("FILE"),
        help_heading("Output"),
        next_line_help(true),
        display_order(0)
    )]
    pub log_file: Option<String>,

    /// Quiet mode
    ///
    /// Switches off progress display and statistics; reports only errors
    #[arg(short, long, conflicts_with("debug"), help_heading("Output"))]
    pub quiet: bool,

    /// Show additional transfer statistics
    #[arg(
        short = 's',
        long,
        alias("stats"),
        conflicts_with("quiet"),
        help_heading("Output"),
        display_order(0)
    )]
    pub statistics: bool,

    /// Enables detailed debug output from the remote endpoint
    /// (this may interfere with transfer speeds)
    #[arg(long, help_heading("Debug"), display_order(0))]
    pub remote_debug: bool,

    /// Enables super-detailed trace output from the remote endpoint
    /// (this may interfere with transfer speeds)
    #[arg(hide = true, long, help_heading("Debug"), display_order(0))]
    pub remote_trace: bool,

    /// Output timing profile data after completion
    #[arg(long, help_heading("Output"), display_order(0))]
    pub profile: bool,

    /// Connects to a remote server but does not actually transfer any files.
    /// This is useful to test that the control channel works and when debugging the negotiated bandwidth parameters (see also `--remote-config`).
    #[arg(long, help_heading("Configuration"), display_order(0))]
    pub dry_run: bool,
    /// Outputs the server's configuration for this connection.
    /// (Unlike `--show-config`, this option does not prevent a file transfer. However, you can do so by selecting `--dry-run` mode.)
    ///
    /// The output shows both the server's _static_ configuration (by reading config files)
    /// and its _final_ configuration (taking account of the client's expressed preferences).
    #[arg(long, help_heading("Configuration"), display_order(0))]
    pub remote_config: bool,

    /// Preserves file modification times and permissions as far as possible.
    #[arg(short, long)]
    pub preserve: bool,

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
        CopyJobSpec::try_new(source, destination, args.preserve)
    }
}

impl Parameters {
    /// A best-effort attempt to extract a single remote host string from the parameters.
    ///
    /// # Returns
    /// If neither source nor dest are present, `Ok("")`
    /// If at most one of source and dest contains a remote host, `Ok(<host>)`
    ///
    /// # Errors
    /// If both source and dest contain a remote host, Err("Only one remote file argument is supported")
    pub(crate) fn remote_host_lossy(&self) -> anyhow::Result<Option<&str>> {
        let src_host = self.source.as_ref().and_then(FileSpec::hostname);
        let dst_host = self.destination.as_ref().and_then(FileSpec::hostname);
        anyhow::ensure!(
            src_host.is_none() || dst_host.is_none(),
            "Only one remote file argument is supported"
        );
        Ok(src_host.or(dst_host))
    }

    /// Extracts the remote username from the jobspec, if there was one.
    /// We do this as a configuration because we allow it to be specified in multiple ways:
    /// * -l username  # same as for ssh/scp
    /// * `user@host:file`
    /// * our configuration file
    pub(crate) fn remote_user_as_config(&self) -> ConfigSource {
        let mut cfg = ConfigSource::new(META_JOBSPEC);
        let src = self.source.as_ref().and_then(FileSpec::remote_user);
        let dest = self.destination.as_ref().and_then(FileSpec::remote_user);
        let either = src.or(dest);
        if let Some(u) = either {
            cfg.add("remote_user", u.into());
        }
        cfg
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use clap::Parser;
    use pretty_assertions::assert_eq;

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
        assert_eq!(params.remote_host_lossy().unwrap(), Some("host"));

        let params = Parameters::parse_from(["test", "source.txt", "user@host:destination.txt"]);
        assert_eq!(params.remote_host_lossy().unwrap(), Some("host"));

        let params = Parameters::parse_from(["test", "source.txt", "destination.txt"]);
        assert_eq!(params.remote_host_lossy().unwrap(), None);
    }

    #[test]
    fn test_copy_job_spec_conversion() {
        let params = Parameters::parse_from(["test", "user@host:source.txt", "destination.txt"]);
        let copy_job_spec = CopyJobSpec::try_from(&params).unwrap();
        assert_eq!(copy_job_spec.source.to_string(), "user@host:source.txt");
        assert_eq!(copy_job_spec.destination.to_string(), "destination.txt");
        assert_eq!(copy_job_spec.remote_host(), "host");
        assert_eq!(copy_job_spec.remote_user().unwrap(), "user");
        assert_eq!(copy_job_spec.user_at_host, "user@host");
    }

    #[test]
    fn there_can_be_only_one_remote() {
        let params =
            Parameters::parse_from(["test", "user@host:source.txt", "user@host:destination.txt"]);
        let _ = CopyJobSpec::try_from(&params).expect_err("but there can be only one!");
        assert!(params.remote_host_lossy().is_err());
    }
}
