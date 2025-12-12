//! Options specific to qcp client-mode
// (c) 2024 Ross Younger

use std::collections::HashSet;
use std::path::{Path, PathBuf};

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
    /// Sources and destination. Provide one or more SOURCE paths followed by a DESTINATION path.
    ///
    /// The last path is always treated as the destination. All preceding paths are treated as sources.
    /// Exactly one side of the transfer (sources or destination) must be remote.
    #[arg(value_name = "PATH", num_args = 0..)]
    pub paths: Vec<FileSpec>,
}

fn basename_of(path: &str) -> anyhow::Result<String> {
    let path = Path::new(path);
    let Some(filename) = path.file_name() else {
        anyhow::bail!("Source path \"{}\" must contain a filename", path.display());
    };
    let filename = filename.to_string_lossy();
    anyhow::ensure!(
        !filename.is_empty(),
        "Source path \"{}\" must contain a filename",
        path.display()
    );
    Ok(filename.to_string())
}

fn join_local_path(base: &str, leaf: &str) -> String {
    if base.is_empty() {
        return leaf.to_string();
    }
    PathBuf::from(base).join(leaf).to_string_lossy().to_string()
}

/// Join a remote path using forward slashes, independent of the client's OS.
///
/// This avoids emitting `\` on Windows clients when the remote host is Unix-like.
fn join_remote_path(base: &str, leaf: &str) -> String {
    if base.is_empty() {
        return leaf.to_string();
    }
    if base.ends_with('/') {
        format!("{base}{leaf}")
    } else {
        format!("{base}/{leaf}")
    }
}

impl TryFrom<&Parameters> for Vec<CopyJobSpec> {
    type Error = anyhow::Error;

    fn try_from(args: &Parameters) -> Result<Self, Self::Error> {
        let (sources, destination) = args.sources_and_destination()?;
        let destination_is_remote = destination.user_at_host.is_some();

        let mut remote_hosts = HashSet::new();
        let mut remote_user: Option<&str> = None;
        for spec in sources.iter().chain(std::iter::once(&destination)) {
            if let Some(host) = spec.hostname() {
                let _ = remote_hosts.insert(host);
            }
            if let Some(user) = spec.remote_user() {
                if let Some(existing) = remote_user {
                    anyhow::ensure!(existing == user, "Only one remote user is supported");
                } else {
                    remote_user = Some(user);
                }
            }
        }
        anyhow::ensure!(remote_hosts.len() <= 1, "Only one remote host is supported");

        let remote_sources: Vec<_> = sources
            .iter()
            .filter(|s| s.user_at_host.is_some())
            .collect();
        if destination_is_remote {
            anyhow::ensure!(
                remote_sources.is_empty(),
                "Only one remote side is supported"
            );
        } else {
            anyhow::ensure!(
                !remote_sources.is_empty(),
                "One file argument must be remote"
            );
        }

        let multiple_sources = sources.len() > 1;
        let mut jobs = Vec::with_capacity(sources.len());

        if destination_is_remote {
            for source in sources {
                anyhow::ensure!(
                    source.user_at_host.is_none(),
                    "Only one remote side is supported"
                );
                let dest_filename = if multiple_sources {
                    let leaf = basename_of(&source.filename)?;
                    join_remote_path(&destination.filename, &leaf)
                } else {
                    destination.filename.clone()
                };
                jobs.push(CopyJobSpec::try_new(
                    source,
                    FileSpec {
                        user_at_host: destination.user_at_host.clone(),
                        filename: dest_filename,
                    },
                    args.preserve,
                )?);
            }
        } else {
            for source in sources {
                anyhow::ensure!(
                    source.user_at_host.is_some(),
                    "Only one remote side is supported"
                );
                let dest_filename = if multiple_sources {
                    let leaf = basename_of(&source.filename)?;
                    join_local_path(&destination.filename, &leaf)
                } else {
                    destination.filename.clone()
                };
                jobs.push(CopyJobSpec::try_new(
                    source,
                    FileSpec {
                        user_at_host: None,
                        filename: dest_filename,
                    },
                    args.preserve,
                )?);
            }
        }
        Ok(jobs)
    }
}

impl Parameters {
    fn sources_and_destination(&self) -> anyhow::Result<(Vec<FileSpec>, FileSpec)> {
        anyhow::ensure!(self.paths.len() >= 2, "source and destination are required");
        let mut paths = self.paths.clone();
        let destination = paths.pop().expect("destination must be present");
        Ok((paths, destination))
    }

    /// A best-effort attempt to extract a single remote host string from the parameters.
    ///
    /// # Returns
    /// If no remote hosts are present, `Ok(None)`
    /// If all remote paths use the same host and only one side of the transfer is remote, `Ok(Some(<host>))`
    ///
    /// # Errors
    /// If remote paths refer to multiple hosts or both the sources _and_ destination are remote
    /// an error is returned.
    pub(crate) fn remote_host_lossy(&self) -> anyhow::Result<Option<&str>> {
        if self.paths.is_empty() {
            return Ok(None);
        }
        let mut host: Option<&str> = None;
        let mut remote_in_sources = false;
        let mut remote_in_destination = false;
        for (idx, spec) in self.paths.iter().enumerate() {
            if let Some(h) = spec.hostname() {
                if let Some(existing) = host {
                    anyhow::ensure!(existing == h, "Only one remote host is supported");
                } else {
                    host = Some(h);
                }
                if idx == self.paths.len() - 1 {
                    remote_in_destination = true;
                } else {
                    remote_in_sources = true;
                }
            }
        }
        anyhow::ensure!(
            !(remote_in_sources && remote_in_destination),
            "Only one remote side is supported"
        );
        Ok(host)
    }

    /// Extracts the remote username from the jobspec, if there was one.
    /// We do this as a configuration because we allow it to be specified in multiple ways:
    /// * -l username  # same as for ssh/scp
    /// * `user@host:file`
    /// * our configuration file
    pub(crate) fn remote_user_as_config(&self) -> ConfigSource {
        let mut cfg = ConfigSource::new(META_JOBSPEC);
        let mut remote_user: Option<&str> = None;
        for spec in &self.paths {
            let user = spec.remote_user();
            if let Some(u) = user {
                if let Some(existing) = remote_user {
                    if existing != u {
                        // Conflicting usernames; we cannot pick one reliably.
                        remote_user = None;
                        break;
                    }
                } else {
                    remote_user = Some(u);
                }
            }
        }
        if let Some(u) = remote_user {
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
    use figment::{Profile, Provider as _};
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
        assert_eq!(params.paths[0].to_string(), "source.txt");
        assert_eq!(params.paths[1].to_string(), "destination.txt");
    }

    #[test]
    fn test_remote_host_lossy() {
        let params = Parameters::parse_from(["test", "user@host:source.txt", "destination.txt"]);
        assert_eq!(params.remote_host_lossy().unwrap(), Some("host"));

        let params = Parameters::parse_from(["test", "source.txt", "user@host:destination.txt"]);
        assert_eq!(params.remote_host_lossy().unwrap(), Some("host"));

        let params = Parameters::parse_from(["test", "source.txt", "destination.txt"]);
        assert_eq!(params.remote_host_lossy().unwrap(), None);

        let params = Parameters::parse_from(["test", "user@host:"]);
        assert_eq!(params.remote_host_lossy().unwrap(), Some("host"));

        let params = Parameters::parse_from(["test", "source.txt"]);
        assert_eq!(params.remote_host_lossy().unwrap(), None);
    }

    #[test]
    fn test_copy_job_spec_conversion() {
        let params = Parameters::parse_from(["test", "user@host:source.txt", "destination.txt"]);
        let specs: Vec<CopyJobSpec> = (&params).try_into().unwrap();
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
        let params =
            Parameters::parse_from(["test", "user@host:source.txt", "user@host:destination.txt"]);
        let _ = <Vec<CopyJobSpec>>::try_from(&params).expect_err("but there can be only one!");
        assert!(params.remote_host_lossy().is_err());
    }

    #[test]
    fn multiple_local_sources_to_remote_destination() {
        let params = Parameters::parse_from(["test", "file1", "file2", "user@host:remote_dir"]);
        let specs: Vec<CopyJobSpec> = (&params).try_into().unwrap();
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
        let params =
            Parameters::parse_from(["test", "user@host:/tmp/a", "user@host:/tmp/b", "downloads"]);
        let specs: Vec<CopyJobSpec> = (&params).try_into().unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(
            specs[0].destination.to_string(),
            join_local_path("downloads", "a")
        );
        assert_eq!(
            specs[1].destination.to_string(),
            join_local_path("downloads", "b")
        );
    }

    #[test]
    fn conflicting_remote_users_rejected() {
        let params =
            Parameters::parse_from(["test", "alice@host:file1", "bob@host:file2", "downloads"]);
        let err = <Vec<CopyJobSpec>>::try_from(&params).unwrap_err();
        assert!(
            err.to_string()
                .contains("Only one remote user is supported")
        );
    }

    #[test]
    fn local_to_local_rejected() {
        let params = Parameters::parse_from(["test", "file1", "downloads"]);
        let err = <Vec<CopyJobSpec>>::try_from(&params).unwrap_err();
        assert!(err.to_string().contains("One file argument must be remote"));
    }

    #[test]
    fn multiple_remote_hosts_rejected() {
        let params = Parameters::parse_from([
            "test",
            "alice@host1:/tmp/a",
            "alice@host2:/tmp/b",
            "downloads",
        ]);
        let err = <Vec<CopyJobSpec>>::try_from(&params).unwrap_err();
        assert!(
            err.to_string()
                .contains("Only one remote host is supported")
        );
    }

    #[test]
    fn remote_destination_with_trailing_slash_is_joined_cleanly() {
        let params = Parameters::parse_from(["test", "file1", "file2", "user@host:remote_dir/"]);
        let specs: Vec<CopyJobSpec> = (&params).try_into().unwrap();
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
        let params = Parameters::parse_from(["test", "file1", "file2", "user@host:"]);
        let specs: Vec<CopyJobSpec> = (&params).try_into().unwrap();
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].destination.to_string(), "user@host:file1");
        assert_eq!(specs[1].destination.to_string(), "user@host:file2");
    }

    #[test]
    fn source_basename_is_required_for_multi_source_copy() {
        let params = Parameters::parse_from(["test", "user@host:", "user@host:/tmp/b", "."]);
        let err = <Vec<CopyJobSpec>>::try_from(&params).unwrap_err();
        assert!(err.to_string().contains("must contain a filename"));
    }

    #[test]
    fn sources_and_destination_requires_two_paths() {
        let params = Parameters::parse_from(["test", "user@host:file1"]);
        let err = <Vec<CopyJobSpec>>::try_from(&params).unwrap_err();
        assert!(
            err.to_string()
                .contains("source and destination are required")
        );
    }

    #[test]
    fn remote_host_lossy_rejects_multiple_hosts() {
        let params =
            Parameters::parse_from(["test", "user@host1:file1", "user@host2:file2", "downloads"]);
        let err = params.remote_host_lossy().unwrap_err();
        assert!(
            err.to_string()
                .contains("Only one remote host is supported")
        );
    }

    #[test]
    fn remote_host_lossy_empty_paths() {
        let params = Parameters::parse_from(["test"]);
        assert_eq!(params.remote_host_lossy().unwrap(), None);
    }

    #[test]
    fn remote_user_as_config_is_set_when_consistent() {
        let params = Parameters::parse_from(["test", "user@host:source.txt", "destination.txt"]);
        let cfg = params.remote_user_as_config();
        let data = cfg.data().unwrap();
        let dict = data.get(&Profile::Global).unwrap();
        assert_eq!(dict.get("remote_user").unwrap().as_str(), Some("user"));
    }

    #[test]
    fn remote_user_as_config_ignored_on_conflict() {
        let params =
            Parameters::parse_from(["test", "alice@host:file1", "bob@host:file2", "downloads"]);
        let cfg = params.remote_user_as_config();
        let data = cfg.data().unwrap();
        let dict = data.get(&Profile::Global).unwrap();
        assert!(dict.get("remote_user").is_none());
    }
}
