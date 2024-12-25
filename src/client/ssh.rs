//! Interaction with ssh configuration
// (c) 2024 Ross Younger

use std::{path::PathBuf, str::FromStr};

use crate::config::ssh::Parser;
use anyhow::{Context, Result};
use tracing::{debug, warn};

use crate::os::{AbstractPlatform as _, Platform};

/// Metadata representing a QCP config file
struct ConfigFile {
    /// The file to read
    path: PathBuf,
    /// if set, this is a user file i.e. ~ expansion is allowed
    user: bool,
    /// if set, warns on various failures and attempts to keep going
    warn_on_error: bool,
}

impl ConfigFile {
    fn for_path(path: PathBuf, user: bool) -> Self {
        Self {
            path,
            user,
            warn_on_error: false,
        }
    }
    fn for_str(path: &str, user: bool, warn_on_error: bool) -> Result<Self> {
        Ok(Self {
            path: PathBuf::from_str(path)?,
            user,
            warn_on_error,
        })
    }

    /// Attempts to resolve a hostname from a single OpenSSH-style config file
    fn resolve_one(&self, host: &str) -> Option<String> {
        let path = &self.path;
        if !std::fs::exists(path).is_ok_and(|b| b) {
            // file could not be verified to exist.
            // This is not intrinsically an error; the user or system file might legitimately not be there.
            // But if this was a file explicitly specified by the user, assume they do care and let them know.
            if self.warn_on_error {
                warn!("ssh-config file {path:?} not found");
            }
            return None;
        }
        let parser = match Parser::for_path(path, self.user) {
            Ok(p) => p,
            Err(e) => {
                // file permissions issue?
                warn!("failed to open {path:?}: {e}");
                return None;
            }
        };
        let data = match parser
            .parse_file_for(Some(host))
            .with_context(|| format!("reading configuration file {path:?}"))
        {
            Ok(data) => data,
            Err(e) => {
                warn!("{e}");
                return None;
            }
        };
        if let Some(s) = data.get("hostname") {
            let result = s.first_arg();
            debug!("Using hostname '{result}' for '{host}' (from {})", s.source);
            Some(result)
        } else {
            None
        }
    }
}

/// Attempts to resolve hostname aliasing from ssh config files.
///
/// ## Arguments
/// * host: the host name alias to look up (matching a 'Host' block in ssh_config)
/// * config_files: The list of ssh config files to use, in priority order.
///
/// If the list is empty, the user's and system's ssh config files will be used.
///
/// ## Returns
/// Some(hostname) if any config file matched.
/// None if no config files matched.
///
/// ## ssh_config features not currently supported
/// * Match patterns
/// * CanonicalizeHostname and friends
#[must_use]
pub fn resolve_host_alias(host: &str, config_files: &[String]) -> Option<String> {
    let files = if config_files.is_empty() {
        let mut v = Vec::new();
        if let Ok(f) = Platform::user_ssh_config() {
            v.push(ConfigFile::for_path(f, true));
        }
        if let Ok(f) = ConfigFile::for_str(Platform::system_ssh_config(), false, false) {
            v.push(f);
        }
        v
    } else {
        config_files
            .iter()
            .flat_map(|s| ConfigFile::for_str(s, true, true))
            .collect()
    };
    for cfg in files {
        let result = cfg.resolve_one(host);
        if result.is_some() {
            return result;
        }
    }
    None
}

#[cfg(test)]
mod test {
    use std::path::Path;

    use super::ConfigFile;
    use crate::util::make_test_tempfile;

    fn resolve_one(path: &Path, user: bool, host: &str) -> Option<String> {
        ConfigFile::for_path(path.to_path_buf(), user).resolve_one(host)
    }

    #[test]
    fn hosts_resolve() {
        let (path, _dir) = make_test_tempfile(
            r"
        Host aaa
            HostName zzz
        Host bbb ccc.ddd
            HostName yyy
        ",
            "test_ssh_config",
        );
        assert!(resolve_one(&path, false, "nope").is_none());
        assert_eq!(resolve_one(&path, false, "aaa").unwrap(), "zzz");
        assert_eq!(resolve_one(&path, false, "bbb").unwrap(), "yyy");
        assert_eq!(resolve_one(&path, false, "ccc.ddd").unwrap(), "yyy");
    }

    #[test]
    fn wildcards_match() {
        let (path, _dir) = make_test_tempfile(
            r"
        Host *.bar
            HostName baz
        Host 10.11.*.13
            # this is a silly example but it shows that wildcards match by IP
            HostName wibble
        Host fr?d
            hostname barney
        ",
            "test_ssh_config",
        );
        assert_eq!(resolve_one(&path, false, "foo.bar").unwrap(), "baz");
        assert_eq!(resolve_one(&path, false, "qux.qix.bar").unwrap(), "baz");
        assert!(resolve_one(&path, false, "qux.qix").is_none());
        assert_eq!(resolve_one(&path, false, "10.11.12.13").unwrap(), "wibble");
        assert_eq!(resolve_one(&path, false, "10.11.0.13").unwrap(), "wibble");
        assert_eq!(resolve_one(&path, false, "10.11.256.13").unwrap(), "wibble"); // yes I know this isn't a real IP address
        assert!(resolve_one(&path, false, "10.11.0.130").is_none());

        assert_eq!(resolve_one(&path, false, "fred").unwrap(), "barney");
        assert_eq!(resolve_one(&path, false, "frid").unwrap(), "barney");
        assert!(resolve_one(&path, false, "freed").is_none());
        assert!(resolve_one(&path, false, "fredd").is_none());
    }
}
