//! Interaction with ssh configuration
// (c) 2024 Ross Younger

use std::{ffi::OsStr, path::PathBuf};

use crate::config::ssh::{Parser, Setting};
use anyhow::Context;
use tracing::{debug, warn};

use crate::os::{AbstractPlatform as _, Platform};

/// Metadata representing an ssh config file
struct SshConfigFile {
    /// The file to read
    path: PathBuf,
    /// if set, this is a user file i.e. ~ expansion is allowed
    user: bool,
    /// if set, warns on various failures and attempts to keep going
    warn_on_error: bool,
}

impl SshConfigFile {
    fn new<S: AsRef<OsStr> + ?Sized>(s: &S, user: bool, warn_on_error: bool) -> Self {
        Self {
            path: s.into(),
            user,
            warn_on_error,
        }
    }

    /// Attempts to resolve a variable from a single OpenSSH-style config file.
    /// Returns None if there was no matching setting.
    fn get(&self, host: &str, key: &str) -> Option<Setting> {
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
                warn!("failed to open {path:?}: {e:?}");
                return None;
            }
        };
        let data = match parser
            .parse_file_for(Some(host))
            .with_context(|| format!("error reading configuration file {}", path.display()))
        {
            Ok(data) => data,
            Err(e) => {
                warn!("{e:?}");
                return None;
            }
        };
        data.get(key).map(std::borrow::ToOwned::to_owned)
    }
}

//////////////////////////////////////////////////////////////////////////////

/// A set of ssh config files
pub(crate) struct SshConfigFiles {
    files: Vec<SshConfigFile>,
}

impl SshConfigFiles {
    pub(crate) fn new<S>(config_files: &[S]) -> Self
    where
        S: AsRef<OsStr>,
    {
        let files = if config_files.is_empty() {
            let mut v = Vec::new();
            if let Some(f) = Platform::user_ssh_config() {
                v.push(SshConfigFile::new(&f, true, false));
            }
            if let Some(p) = Platform::system_ssh_config() {
                let f = SshConfigFile::new(&p, false, false);
                v.push(f);
            }
            v
        } else {
            config_files
                .iter()
                .map(|s| SshConfigFile::new(s, true, true))
                .collect()
        };

        Self { files }
    }

    /// Accessor for tests
    #[cfg(test)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn get_files(&self) -> &Vec<SshConfigFile> {
        &self.files
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
    pub(crate) fn resolve_host_alias(&self, host: &str) -> Option<String> {
        self.get(host, "hostname")
            .inspect(|s| {
                debug!(
                    "Using hostname '{}' for '{host}' (from {})",
                    s.first_arg(),
                    s.source
                );
            })
            .map(|s| s.first_arg())
    }

    /// Generic access to any key in ssh config files.
    /// This is moderately expensive as we have to walk through the config files and evaluate all matching sections.
    ///
    /// ## Arguments:
    /// * host: The host we are interested in. Wildcards will be applied.
    /// * key: The config key we are interested in. This must be canonicalised (all lower case, no spaces or underscores).
    #[must_use]
    pub(crate) fn get(&self, host: &str, key: &str) -> Option<Setting> {
        self.files.iter().find_map(|c| c.get(host, key))
    }
}

//////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use std::{ffi::OsStr, path::PathBuf};

    use super::SshConfigFiles;
    use crate::{client::ssh::SshConfigFile, util::littertray::LitterTray};

    fn resolve_one<P: AsRef<OsStr>>(path: P, host: &str) -> Option<String> {
        let files = SshConfigFiles::new(&[path.as_ref()]);
        files.resolve_host_alias(host)
    }

    #[test]
    fn hosts_resolve() {
        LitterTray::try_with(|tray| {
            let path = "test_ssh_config";
            let _ = tray.create_text(
                path,
                r"
        Host aaa
            HostName zzz
        Host bbb ccc.ddd
            HostName yyy
            ",
            )?;
            assert!(resolve_one(path, "nope").is_none());
            assert_eq!(resolve_one(path, "aaa").unwrap(), "zzz");
            assert_eq!(resolve_one(path, "bbb").unwrap(), "yyy");
            assert_eq!(resolve_one(path, "ccc.ddd").unwrap(), "yyy");
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn wildcards_match() {
        LitterTray::try_with(|tray| {
            let path = "test_ssh_config";
            let _ = tray.create_text(
                path,
                r"
        Host *.bar
            HostName baz
        Host 10.11.*.13
            # this is a silly example but it shows that wildcards match by IP
            HostName wibble
        Host fr?d
            hostname barney
        ",
            )?;
            assert_eq!(resolve_one(path, "foo.bar").unwrap(), "baz");
            assert_eq!(resolve_one(path, "qux.qix.bar").unwrap(), "baz");
            assert!(resolve_one(path, "qux.qix").is_none());
            assert_eq!(resolve_one(path, "10.11.12.13").unwrap(), "wibble");
            assert_eq!(resolve_one(path, "10.11.0.13").unwrap(), "wibble");
            assert_eq!(resolve_one(path, "10.11.256.13").unwrap(), "wibble"); // yes I know this isn't a real IP address
            assert!(resolve_one(path, "10.11.0.130").is_none());

            assert_eq!(resolve_one(path, "fred").unwrap(), "barney");
            assert_eq!(resolve_one(path, "frid").unwrap(), "barney");
            assert!(resolve_one(path, "freed").is_none());
            assert!(resolve_one(path, "fredd").is_none());
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn no_such_file() {
        let path = PathBuf::from("/no-such-file--------------");
        let s = SshConfigFile::new(&path, false, true);
        // testing that this doesn't panic
        assert!(s.get("", "hostname").is_none());
    }
    #[test]
    fn file_permissions() {
        let path = PathBuf::from("/dev/console");
        let s = SshConfigFile::new(&path, false, false);
        // testing that this doesn't panic
        assert!(s.get("", "hostname").is_none());
    }
    #[test]
    fn reading_failed() {
        let path = "myfile";
        let contents = format!("include {path:?}");
        LitterTray::try_with(|tray| {
            let _f = tray.create_text(path, &contents)?;
            let s = SshConfigFile::new(&path, false, false);
            // testing that this doesn't panic
            assert!(s.get("", "hostname").is_none());
            Ok(())
        })
        .unwrap();
    }
    #[test]
    fn empty_fileset() {
        let f = SshConfigFiles::new::<&str>(&[]);
        let files = f.get_files();
        assert!(files.len() == 2);
    }
}
