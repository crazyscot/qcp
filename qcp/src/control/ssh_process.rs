//! Subprocess management (client side)
// (c) 2024-2025 Ross Younger

use std::process::Stdio;
use tokio::io::BufReader;

use anyhow::{Context as _, Result};
use indicatif::MultiProgress;
use tokio::io::AsyncBufReadExt;
use tracing::debug;

use crate::client::Parameters;
use crate::config::{Configuration, Configuration_Optional};
use crate::{cli::styles::maybe_strip_color, protocol::control::ConnectionType};

use crate::util::process::ProcessWrapper;

fn ssh_cli_args(
    connection_type: ConnectionType,
    ssh_hostname: &str,
    config: &Configuration_Optional,
    remote_trace: bool,
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

    if remote_trace {
        args.push("RUST_LOG=qcp=trace".to_owned());
    }

    // Subsystem or process (must appear after hostname!)
    if config.ssh_subsystem.unwrap_or(false) {
        args.extend_from_slice(&["-s".to_owned(), "qcp".to_owned()]);
    } else {
        let remote_qcp_binary = config
            .remote_qcp_binary
            .as_ref()
            .unwrap_or(&defaults.remote_qcp_binary)
            .clone();
        args.push(remote_qcp_binary);
        args.push("--server".to_owned());
    }

    args
}

/// Constructor
pub(crate) fn create(
    display: &MultiProgress,
    working_config: &Configuration_Optional,
    parameters: &Parameters,
    ssh_hostname: &str,
    connection_type: ConnectionType,
) -> Result<ProcessWrapper> {
    let defaults = Configuration::system_default();

    let mut server = tokio::process::Command::new(
        working_config
            .ssh
            .as_deref()
            .unwrap_or_else(|| &defaults.ssh),
    );

    let _ = server
        .args(ssh_cli_args(
            connection_type,
            ssh_hostname,
            working_config,
            parameters.remote_trace,
        ))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    if !parameters.quiet {
        let _ = server.stderr(Stdio::piped());
    } // else inherit
    debug!("spawning command: {:?}", server);
    let mut wrapper = ProcessWrapper::spawn(server)
        .context("Could not launch control connection to remote server")?;

    // Whatever the remote outputs, send it to our output in a way that doesn't mess things up.
    if !parameters.quiet {
        let stderr = wrapper.stderr();
        let Some(stderr) = stderr else {
            anyhow::bail!("could not get stderr of remote process");
        };
        let cloned = display.clone();
        let _reader = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                let line = maybe_strip_color(&line);
                // Calling cloned.println() sometimes messes up; there seems to be a concurrency issue.
                // But we don't need to worry too much about that. Just write it out.
                cloned.suspend(|| eprintln!("{line}"));
            }
        });
    }
    Ok(wrapper)
}

#[cfg(test)]
#[cfg(unix)]
#[cfg_attr(coverage_nightly, coverage(off))]
/// Creates a fake client for testing purposes.
/// It returns a given output stream.
pub(crate) fn create_fake<B>(data: &B) -> ProcessWrapper
where
    B: AsRef<[u8]> + Send + 'static,
{
    use std::fmt::Write;

    let mut encoded = String::new();
    for byte in data.as_ref() {
        // Encode each byte as an octal number (osx /bin/echo doesn't grok hex)
        write!(encoded, "\\0{byte:o}").expect("failed to write to encoded string");
    }
    eprintln!("Fake client will echo -e {encoded}");

    let mut process = tokio::process::Command::new("echo");
    let _ = process.args(["-en", &encoded]);
    ProcessWrapper::spawn(process).expect("failed to start fake ssh client")
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::{Configuration_Optional, ConnectionType, ssh_cli_args};

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

    #[test]
    fn connection_type() {
        let cfg = Configuration_Optional::default();
        // ipv4
        let args = ssh_cli_args(ConnectionType::Ipv4, "", &cfg, false);
        assert!(vec_contains(&args, "-4"));
        assert!(!vec_contains(&args, "-6"));
        // ipv6
        let args = ssh_cli_args(ConnectionType::Ipv6, "", &cfg, false);
        assert!(vec_contains(&args, "-6"));
        assert!(!vec_contains(&args, "-4"));
    }

    #[test]
    fn username() {
        // negative case
        let args = ssh_cli_args(
            ConnectionType::Ipv4,
            "",
            &Configuration_Optional::default(),
            false,
        );
        assert!(!vec_contains(&args, "-l"));

        // positive case
        let cfg1 = Configuration_Optional {
            remote_user: Some("xyzy".to_owned()),
            ..Default::default()
        };
        let args = ssh_cli_args(ConnectionType::Ipv4, "", &cfg1, false);
        assert!(vec_subslice_strings(&args, &["-l", "xyzy"]));
    }

    #[test]
    fn hostname_ssh_opts() {
        let xopts = ["--abc", "def", "--ghi", "jkl"];
        let cfg1 = Configuration_Optional {
            ssh_options: Some(xopts.map(String::from).to_vec()),
            ..Default::default()
        };
        let args = ssh_cli_args(ConnectionType::Ipv4, "my_host", &cfg1, false);
        assert!(vec_contains(&args, "my_host"));
        assert!(vec_subslice_strings(&args, &xopts));
    }

    #[test]
    fn subsystem_mode() {
        let host = "myserver";
        let args = ssh_cli_args(
            ConnectionType::Ipv4,
            host,
            &Configuration_Optional::default(),
            false,
        );
        assert!(vec_subslice_strings(&args, &["qcp", "--server"]));

        let cfg1 = Configuration_Optional {
            ssh_subsystem: Some(true),
            ..Default::default()
        };
        let args1 = ssh_cli_args(ConnectionType::Ipv4, host, &cfg1, false);
        assert!(vec_subslice_strings(&args1, &["-s", "qcp"]));
    }

    #[test]
    fn remote_qcp_binary_override() {
        let cfg1 = Configuration_Optional {
            remote_qcp_binary: Some("/opt/qcp/bin/qcp".to_owned()),
            ..Default::default()
        };
        let args = ssh_cli_args(ConnectionType::Ipv4, "my_host", &cfg1, false);
        assert!(vec_subslice_strings(
            &args,
            &["/opt/qcp/bin/qcp", "--server"]
        ));

        // Remote path should not leak into subsystem mode
        let cfg2 = Configuration_Optional {
            remote_qcp_binary: Some("/opt/qcp/bin/qcp".to_owned()),
            ssh_subsystem: Some(true),
            ..Default::default()
        };
        let args2 = ssh_cli_args(ConnectionType::Ipv4, "my_host", &cfg2, false);
        assert!(vec_subslice_strings(&args2, &["-s", "qcp"]));
        assert!(!vec_contains(&args2, "/opt/qcp/bin/qcp"));
    }
}
