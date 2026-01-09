//! Options specific to qcp client-mode
// (c) 2024 Ross Younger

use clap::Parser;

#[derive(Debug, Parser, Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)]
/// Client-side options which may be provided on the command line, but are not persistent configuration options.
pub struct Parameters {
    /// Enable detailed debug output
    ///
    /// This has the same effect as setting `RUST_LOG=qcp=debug` in the environment.
    /// If present, `RUST_LOG` overrides this option.
    #[arg(long, help_heading("Debug"), display_order(10))]
    pub debug: bool,

    /// Quiet mode
    ///
    /// Switches off progress display and statistics; reports only errors
    #[arg(short, long, conflicts_with("debug"), display_order(0))]
    pub quiet: bool,

    /// Show additional transfer statistics
    #[arg(
        short = 's',
        long,
        alias("stats"),
        conflicts_with("quiet"),
        display_order(0)
    )]
    pub statistics: bool,

    /// Enables detailed debug output from the remote endpoint
    /// (this may interfere with transfer speeds)
    #[arg(long, help_heading("Debug"), display_order(10))]
    pub remote_debug: bool,

    /// Enables super-detailed trace output from the remote endpoint
    /// (this may interfere with transfer speeds)
    #[arg(hide = true, long, help_heading("Debug"), display_order(10))]
    pub remote_trace: bool,

    /// Output timing profile data after completion
    #[arg(long, display_order(0))]
    pub profile: bool,

    /// Connects to a remote server but does not actually transfer any files.
    ///
    /// This is useful to test that the control channel works and when debugging the negotiated bandwidth parameters (see also `--remote-config`).
    #[arg(long, help_heading("Debug"), display_order(10))]
    pub dry_run: bool,
    /// Outputs the server's configuration for this connection.
    ///
    /// Unlike `--show-config`, this option does not prevent a file transfer. However, you can do so by selecting `--dry-run` mode.
    ///
    /// The output shows both the server's _static_ configuration (by reading config files)
    /// and its _final_ configuration (taking account of the client's expressed preferences).
    #[arg(long, help_heading("Debug"), display_order(10))]
    pub remote_config: bool,

    /// Preserves file/directory permissions and file modification times as far as possible.
    ///
    /// Directory modification times are not preserved. This is because they are OS-specific and not well defined.
    #[arg(short, long, display_order(0))]
    pub preserve: bool,

    /// Copies entire directories recursively, following symbolic links.
    ///
    /// Behaviour is intended to match that of scp.
    ///
    /// ### Get
    /// * If there are multiple sources, the destination must exist; the sources (including outer directory names) are copied into it.
    /// * If there is one source:
    ///   - if the destination exists, the source (file or directory) is copied into it.
    ///   - if the destination does not exist, it is created; then the *contents* of the source are copied into it.
    /// * if the destination does not exist, there can be only one source
    ///
    /// ### Put
    /// * If the destination ends in '/', the source directory goes _into_ the destination
    /// * If not, the *contents* of the source go into the destination.
    ///
    #[arg(
        short,
        long,
        display_order(0),
        long_help(
            "Copies entire directories recursively, following symbolic links.\n\nBehaviour is intended to match scp."
        )
    )]
    pub recurse: bool,
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::cli::CliArgs;
    use crate::util::path;
    use clap::Parser;
    use figment::{Profile, Provider as _};
    use pretty_assertions::assert_eq;

    #[test]
    fn test_debug_option() {
        let params = Parameters::parse_from(["test", "--debug"]);
        assert!(params.debug);
    }

    #[test]
    fn test_log_file_option() {
        let args = CliArgs::parse_from(["test", "--log-file", "log.txt"]);
        assert_eq!(args.log_file, Some("log.txt".to_string()));
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
        let args = CliArgs::parse_from(["test", "source.txt", "destination.txt"]);
        assert_eq!(args.paths[0].to_string(), "source.txt");
        assert_eq!(args.paths[1].to_string(), "destination.txt");
    }

    #[test]
    fn test_remote_host_lossy() {
        let args = CliArgs::parse_from(["test", "user@host:source.txt", "destination.txt"]);
        assert_eq!(args.remote_host_lossy().unwrap(), Some("host"));

        let args = CliArgs::parse_from(["test", "source.txt", "user@host:destination.txt"]);
        assert_eq!(args.remote_host_lossy().unwrap(), Some("host"));

        let args = CliArgs::parse_from(["test", "source.txt", "destination.txt"]);
        assert_eq!(args.remote_host_lossy().unwrap(), None);

        let args = CliArgs::parse_from(["test", "user@host:"]);
        assert_eq!(args.remote_host_lossy().unwrap(), Some("host"));

        let args = CliArgs::parse_from(["test", "source.txt"]);
        assert_eq!(args.remote_host_lossy().unwrap(), None);
    }

    #[test]
    fn test_copy_job_spec_conversion() {
        let args = CliArgs::parse_from(["test", "user@host:source.txt", "destination.txt"]);
        let (ok, specs) = args.jobspecs().unwrap();
        assert!(ok);
        assert_eq!(specs.len(), 1);
        let copy_job_spec = specs.first().unwrap();
        assert_eq!(copy_job_spec.source.to_string(), "user@host:source.txt");
        assert_eq!(copy_job_spec.destination.to_string(), "destination.txt");
        assert_eq!(copy_job_spec.remote_host(), "host");
        assert_eq!(copy_job_spec.remote_user().unwrap(), "user");
        assert_eq!(copy_job_spec.user_at_host, "user@host");
    }

    #[test]
    fn there_can_be_only_one_remote() {
        let args =
            CliArgs::parse_from(["test", "user@host:source.txt", "user@host:destination.txt"]);
        let _ = args.jobspecs().expect_err("but there can be only one!");
        assert!(args.remote_host_lossy().is_err());
    }

    #[test]
    fn multiple_local_sources_to_remote_destination() {
        let args = CliArgs::parse_from(["test", "file1", "file2", "user@host:remote_dir"]);
        let (ok, specs) = args.jobspecs().unwrap();
        assert!(ok);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].source.to_string(), "file1");
        assert_eq!(
            specs[0].destination.to_string(),
            "user@host:remote_dir/file1"
        );
        assert_eq!(
            specs[1].destination.to_string(),
            "user@host:remote_dir/file2"
        );
    }

    #[test]
    fn multiple_remote_sources_to_local_destination() {
        let args =
            CliArgs::parse_from(["test", "user@host:/tmp/a", "user@host:/tmp/b", "downloads"]);
        let (ok, specs) = args.jobspecs().unwrap();
        assert!(ok);
        assert_eq!(specs.len(), 2);
        assert_eq!(
            specs[0].destination.to_string(),
            path::join_local("downloads", "a")
        );
        assert_eq!(
            specs[1].destination.to_string(),
            path::join_local("downloads", "b")
        );
    }

    #[test]
    fn conflicting_remote_users_rejected() {
        let args = CliArgs::parse_from(["test", "alice@host:file1", "bob@host:file2", "downloads"]);
        let err = args.jobspecs().unwrap_err();
        assert!(
            err.to_string()
                .contains("Only one remote user is supported")
        );
    }

    #[test]
    fn local_to_local_rejected() {
        let args = CliArgs::parse_from(["test", "file1", "downloads"]);
        let err = args.jobspecs().unwrap_err();
        assert!(err.to_string().contains("One file argument must be remote"));
    }

    #[test]
    fn multiple_remote_hosts_rejected() {
        let args = CliArgs::parse_from([
            "test",
            "alice@host1:/tmp/a",
            "alice@host2:/tmp/b",
            "downloads",
        ]);
        let err = args.jobspecs().unwrap_err();
        assert!(
            err.to_string()
                .contains("Only one remote host is supported")
        );
    }

    #[test]
    fn remote_destination_with_trailing_slash_is_joined_cleanly() {
        let args = CliArgs::parse_from(["test", "file1", "file2", "user@host:remote_dir/"]);
        let (ok, specs) = args.jobspecs().unwrap();
        assert!(ok);
        assert_eq!(specs.len(), 2);
        assert_eq!(
            specs[0].destination.to_string(),
            "user@host:remote_dir/file1"
        );
        assert_eq!(
            specs[1].destination.to_string(),
            "user@host:remote_dir/file2"
        );
    }

    #[test]
    fn remote_destination_home_dir_is_supported() {
        let args = CliArgs::parse_from(["test", "file1", "file2", "user@host:"]);
        let (ok, specs) = args.jobspecs().unwrap();
        assert!(ok);
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].destination.to_string(), "user@host:file1");
        assert_eq!(specs[1].destination.to_string(), "user@host:file2");
    }

    #[test]
    fn source_basename_is_required_for_multi_source_copy() {
        let args = CliArgs::parse_from(["test", "user@host:", "user@host:/tmp/b", "."]);
        let err = args.jobspecs().unwrap_err();
        assert!(err.to_string().contains("must contain a filename"));
    }

    #[test]
    fn sources_and_destination_requires_two_paths() {
        let args = CliArgs::parse_from(["test", "user@host:file1"]);
        let err = args.jobspecs().unwrap_err();
        assert!(
            err.to_string()
                .contains("source and destination are required")
        );
    }

    #[test]
    fn remote_host_lossy_rejects_multiple_hosts() {
        let params =
            CliArgs::parse_from(["test", "user@host1:file1", "user@host2:file2", "downloads"]);
        let err = params.remote_host_lossy().unwrap_err();
        assert!(
            err.to_string()
                .contains("Only one remote host is supported")
        );
    }

    #[test]
    fn remote_host_lossy_empty_paths() {
        let params = CliArgs::parse_from(["test"]);
        assert_eq!(params.remote_host_lossy().unwrap(), None);
    }

    #[test]
    fn remote_user_as_config_is_set_when_consistent() {
        let params = CliArgs::parse_from(["test", "user@host:source.txt", "destination.txt"]);
        let cfg = params.remote_user_as_config();
        let data = cfg.data().unwrap();
        let dict = data.get(&Profile::Global).unwrap();
        assert_eq!(dict.get("remote_user").unwrap().as_str(), Some("user"));
    }

    #[test]
    fn remote_user_as_config_ignored_on_conflict() {
        let params =
            CliArgs::parse_from(["test", "alice@host:file1", "bob@host:file2", "downloads"]);
        let cfg = params.remote_user_as_config();
        let data = cfg.data().unwrap();
        let dict = data.get(&Profile::Global).unwrap();
        assert!(dict.get("remote_user").is_none());
    }

    #[test]
    fn join_local_path_with_empty_base_returns_leaf() {
        assert_eq!(path::join_local("", "file"), "file");
    }

    #[test]
    fn single_local_source_to_remote_destination_is_supported() {
        let args = CliArgs::parse_from(["test", "file1", "user@host:remote_file"]);
        let (ok, specs) = args.jobspecs().unwrap();
        assert!(ok);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].destination.to_string(), "user@host:remote_file");
    }

    #[test]
    fn remote_user_as_config_is_not_set_when_no_user_is_provided() {
        let args = CliArgs::parse_from(["test", "host:source.txt", "destination.txt"]);
        let cfg = args.remote_user_as_config();
        let data = cfg.data().unwrap();
        let dict = data.get(&Profile::Global).unwrap();
        assert!(dict.get("remote_user").is_none());
    }

    #[test]
    fn mixed_local_and_remote_sources_to_local_destination_rejected() {
        let args = CliArgs::parse_from(["test", "file1", "user@host:/tmp/b", "downloads"]);
        let err = args.jobspecs().unwrap_err();
        assert!(
            err.to_string()
                .contains("Only one remote side is supported")
        );
    }

    #[test]
    fn remote_host_lossy_allows_multiple_sources_same_host() {
        let params =
            CliArgs::parse_from(["test", "user@host:file1", "user@host:file2", "downloads"]);
        assert_eq!(params.remote_host_lossy().unwrap(), Some("host"));
    }

    #[test]
    fn remote_user_as_config_allows_multiple_paths_same_user() {
        let params =
            CliArgs::parse_from(["test", "alice@host:file1", "alice@host:file2", "downloads"]);
        let cfg = params.remote_user_as_config();
        let data = cfg.data().unwrap();
        let dict = data.get(&Profile::Global).unwrap();
        assert_eq!(dict.get("remote_user").unwrap().as_str(), Some("alice"));
    }
}
