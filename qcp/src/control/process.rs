//! Subprocess management (client side)
// (c) 2024-2025 Ross Younger

use std::process::Stdio;
use tokio::{
    io::BufReader,
    process::{ChildStdin, ChildStdout},
};

use anyhow::{Context as _, Result, anyhow};
use indicatif::MultiProgress;
use tokio::io::AsyncBufReadExt;
use tracing::{debug, warn};

use crate::protocol::control::ConnectionType;
use crate::{client::Parameters, protocol::common::SendReceivePair};
use crate::{
    config::{Configuration, Configuration_Optional, Manager},
    protocol::common::{ReceivingStream, SendingStream},
};

/// A child process (usually ssh) that implements the connection to the remote
#[derive(Debug)]
pub(crate) struct Ssh {
    process: tokio::process::Child,
}

impl Drop for Ssh {
    fn drop(&mut self) {
        // Tidy up as best we can.
        if let Ok(Some(_)) = self.process.try_wait() {
            return;
        }
        let _ = self
            .process
            .start_kill()
            .map_err(|e| warn!("killing connection process: {e}"));
        let _ = self
            .process
            .try_wait()
            .map_err(|e| warn!("reaping connection process: {e}"));
    }
}

impl SendingStream for ChildStdin {}
impl ReceivingStream for ChildStdout {}

impl Ssh {
    /// A reasonably controlled shutdown.
    /// (If you want to be rough, simply drop the [`Channel`].)
    pub(crate) async fn close(&mut self) -> Result<()> {
        // wait() closes the child process stdin
        let _ = self.process.wait().await?;
        Ok(())
    }

    /// Accessor for the communication channels.
    /// Note that this can only be called once; future calls error.
    pub(crate) fn stream_pair(&mut self) -> Result<SendReceivePair<ChildStdin, ChildStdout>> {
        let sp = SendReceivePair::from((
            self.process
                .stdin
                .take()
                .ok_or_else(|| anyhow!("could not access process stdin"))?,
            self.process
                .stdout
                .take()
                .ok_or_else(|| anyhow!("could not access process stdout"))?,
        ));
        Ok(sp)
    }

    fn ssh_cli_args(
        connection_type: ConnectionType,
        ssh_hostname: &str,
        config: &Configuration_Optional,
    ) -> Vec<String> {
        let mut args = Vec::new();
        let defaults = Configuration::system_default();

        // Connection type
        args.push(
            match connection_type {
                ConnectionType::Ipv4 => "-4",
                ConnectionType::Ipv6 => "-6",
            }
            .to_owned(),
        );

        // Remote user
        if let Some(username) = &config.remote_user {
            // N.B. remote_user in config may be populated as a result of user@host syntax on the command line.
            // That takes priority over any value in config (hence, also over the --remote-user option, which would arguably be an error if it was inconsistent).
            if !username.is_empty() {
                args.push("-l".to_owned());
                args.push(username.clone());
            }
        }

        // Other SSH options
        args.extend_from_slice(config.ssh_options.as_ref().unwrap_or(&defaults.ssh_options));

        // Hostname
        args.push(ssh_hostname.to_owned());

        // Subsystem or process (must appear after hostname!)
        args.extend_from_slice(
            &if config.ssh_subsystem.unwrap_or(false) {
                ["-s", "qcp"]
            } else {
                ["qcp", "--server"]
            }
            .map(String::from),
        );

        args
    }

    /// Constructor
    pub(crate) fn new(
        display: &MultiProgress,
        manager: &Manager,
        parameters: &Parameters,
        ssh_hostname: &str,
        connection_type: ConnectionType,
    ) -> Result<Self> {
        let working_config = manager.get::<Configuration_Optional>().unwrap_or_default();
        let defaults = Configuration::system_default();

        let mut server = tokio::process::Command::new(
            working_config
                .ssh
                .as_deref()
                .unwrap_or_else(|| &defaults.ssh),
        );

        let _ = server
            .args(Self::ssh_cli_args(
                connection_type,
                ssh_hostname,
                &working_config,
            ))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true);
        if !parameters.quiet {
            let _ = server.stderr(Stdio::piped());
        } // else inherit
        debug!("spawning command: {:?}", server);
        let mut process = server
            .spawn()
            .context("Could not launch control connection to remote server")?;

        // Whatever the remote outputs, send it to our output in a way that doesn't mess things up.
        if !parameters.quiet {
            let stderr = process.stderr.take();
            let Some(stderr) = stderr else {
                anyhow::bail!("could not get stderr of remote process");
            };
            let cloned = display.clone();
            let _reader = tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    // Calling cloned.println() sometimes messes up; there seems to be a concurrency issue.
                    // But we don't need to worry too much about that. Just write it out.
                    cloned.suspend(|| eprintln!("{line}"));
                }
            });
        }
        Ok(Self { process })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use indicatif::MultiProgress;

    use crate::control::ControlChannel;
    use crate::{client::Parameters, config::Manager};

    use super::{Configuration_Optional, ConnectionType, Ssh};

    fn vec_contains(v: &[String], s: &str) -> bool {
        v.iter().any(|x| x == s)
    }

    // this is O(n^2) but that doesn't matter as we're only using it for short slices
    fn vec_subslice<T: PartialEq>(mut haystack: &[T], needle: &[T]) -> bool {
        if needle.is_empty() {
            return true;
        }
        while !haystack.is_empty() {
            if haystack.starts_with(needle) {
                return true;
            }
            haystack = &haystack[1..];
        }
        false
    }

    // this is O(n^2) but that doesn't matter as we're only using it for short slices
    fn vec_subslice_strings(haystack: &[String], needle1: &[&str]) -> bool {
        let needle = needle1.iter().map(|s| String::from(*s)).collect::<Vec<_>>();
        vec_subslice(haystack, &needle)
    }

    #[tokio::test]
    async fn ssh_no_such_host() {
        let mp = MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden());
        let manager = Manager::without_files(None);
        let params = Parameters::default();
        let mut child = Ssh::new(
            &mp,
            &manager,
            &params,
            "no-such-host.invalid",
            crate::protocol::control::ConnectionType::Ipv4,
        )
        .unwrap();

        // There isn't, at present, a signal that the ssh connection failed. But we see it soon enough when we look for the banner.
        let mut control = ControlChannel::new(child.stream_pair().unwrap());
        let e = control.wait_for_banner().await.unwrap_err();
        assert!(e.to_string().contains("failed to connect"));
        child.close().await.unwrap();
        drop(child);
    }

    #[test]
    fn connection_type() {
        let cfg = Configuration_Optional::default();
        // ipv4
        let args = Ssh::ssh_cli_args(ConnectionType::Ipv4, "", &cfg);
        assert!(vec_contains(&args, "-4"));
        assert!(!vec_contains(&args, "-6"));
        // ipv6
        let args = Ssh::ssh_cli_args(ConnectionType::Ipv6, "", &cfg);
        assert!(vec_contains(&args, "-6"));
        assert!(!vec_contains(&args, "-4"));
    }

    #[test]
    fn username() {
        // negative case
        let args = Ssh::ssh_cli_args(ConnectionType::Ipv4, "", &Configuration_Optional::default());
        assert!(!vec_contains(&args, "-l"));

        // positive case
        let cfg1 = Configuration_Optional {
            remote_user: Some("xyzy".to_owned()),
            ..Default::default()
        };
        let args = Ssh::ssh_cli_args(ConnectionType::Ipv4, "", &cfg1);
        assert!(vec_subslice_strings(&args, &["-l", "xyzy"]));
    }

    #[test]
    fn hostname_ssh_opts() {
        let xopts = ["--abc", "def", "--ghi", "jkl"];
        let cfg1 = Configuration_Optional {
            ssh_options: Some(xopts.map(String::from).to_vec()),
            ..Default::default()
        };
        let args = Ssh::ssh_cli_args(ConnectionType::Ipv4, "my_host", &cfg1);
        assert!(vec_contains(&args, "my_host"));
        assert!(vec_subslice_strings(&args, &xopts));
    }

    #[test]
    fn subsystem_mode() {
        let host = "myserver";
        let args = Ssh::ssh_cli_args(
            ConnectionType::Ipv4,
            host,
            &Configuration_Optional::default(),
        );
        assert!(vec_subslice_strings(&args, &["qcp", "--server"]));

        let cfg1 = Configuration_Optional {
            ssh_subsystem: Some(true),
            ..Default::default()
        };
        let args1 = Ssh::ssh_cli_args(ConnectionType::Ipv4, host, &cfg1);
        assert!(vec_subslice_strings(&args1, &["-s", "qcp"]));
    }
}
