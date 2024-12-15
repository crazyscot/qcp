//! Interaction with ssh configuration
// (c) 2024 Ross Younger

use std::path::PathBuf;

use crate::config::ssh::Parser;
use anyhow::Context;
use tracing::{debug, warn};

use crate::os::{AbstractPlatform as _, Platform};

/// Attempts to resolve a hostname from a single OpenSSH-style config file
fn resolve_one(path: &PathBuf, user_config_file: bool, host: &str) -> Option<String> {
    if !std::fs::exists(path).is_ok_and(|b| b) {
        // file could not be verified to exist. this is not intrinsically an error; keep quiet
        return None;
    }
    let mut parser = match Parser::for_path(path, user_config_file) {
        Ok(p) => p,
        Err(e) => {
            // file permissions issue?
            warn!("failed to open {path:?}: {e}");
            return None;
        }
    };
    let data = match parser
        .parse_file_for(host)
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

/// Attempts to resolve hostname aliasing from the user's and system's ssh config files to resolve aliasing.
///
/// ## Returns
/// Some(hostname) if any config file matched.
/// None if no config files matched.
///
/// ## ssh_config features not currently supported
/// * Match patterns
/// * CanonicalizeHostname and friends
#[must_use]
pub fn resolve_host_alias(host: &str) -> Option<String> {
    let f = Platform::user_ssh_config().map(|pb| resolve_one(&pb, true, host));
    if let Ok(Some(s)) = f {
        return Some(s);
    }

    resolve_one(&PathBuf::from(Platform::system_ssh_config()), false, host)
}

#[cfg(test)]
mod test {
    use super::resolve_one;
    use crate::util::make_test_tempfile;

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
