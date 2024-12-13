//! Interaction with ssh configuration
// (c) 2024 Ross Younger

use std::{fs::File, io::BufReader};

use ssh2_config::{ParseRule, SshConfig};
use tracing::{debug, warn};

use crate::os::{AbstractPlatform as _, Platform};

/// Attempts to resolve a hostname from a single OpenSSH-style config file
///
/// If `path` is None, uses the default user ssh config file.
fn resolve_one(path: Option<&str>, host: &str) -> Option<String> {
    let source = path.unwrap_or("~/.ssh/config");
    let result = match path {
        Some(p) => {
            let mut reader = match File::open(p) {
                Ok(f) => BufReader::new(f),
                Err(e) => {
                    // This is not automatically an error, as the file might not exist.
                    debug!("Unable to read {p}; continuing without. {e}");
                    return None;
                }
            };
            SshConfig::default().parse(&mut reader, ParseRule::ALLOW_UNKNOWN_FIELDS)
        }
        None => SshConfig::parse_default_file(ParseRule::ALLOW_UNKNOWN_FIELDS),
    };
    let cfg = match result {
        Ok(cfg) => cfg,
        Err(e) => {
            warn!("Unable to parse {source}; continuing without. [{e}]");
            return None;
        }
    };

    cfg.query(host).host_name.inspect(|h| {
        debug!("Using hostname '{h}' for '{host}' (from {source})");
    })
}

/// Attempts to resolve hostname aliasing from the user's and system's ssh config files to resolve aliasing.
///
/// ## Returns
/// Some(hostname) if any config file matched.
/// None if no config files matched.
///
/// ## ssh_config features not currently supported
/// * Include directives
/// * Match patterns
/// * CanonicalizeHostname and friends
#[must_use]
pub fn resolve_host_alias(host: &str) -> Option<String> {
    let files = vec![None, Some(Platform::system_ssh_config())];
    files.into_iter().find_map(|it| resolve_one(it, host))
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
        let f = path.to_string_lossy().to_string();
        assert!(resolve_one(Some(&f), "nope").is_none());
        assert_eq!(resolve_one(Some(&f), "aaa").unwrap(), "zzz");
        assert_eq!(resolve_one(Some(&f), "bbb").unwrap(), "yyy");
        assert_eq!(resolve_one(Some(&f), "ccc.ddd").unwrap(), "yyy");
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
        let f = path.to_string_lossy().to_string();
        assert_eq!(resolve_one(Some(&f), "foo.bar").unwrap(), "baz");
        assert_eq!(resolve_one(Some(&f), "qux.qix.bar").unwrap(), "baz");
        assert!(resolve_one(Some(&f), "qux.qix").is_none());
        assert_eq!(resolve_one(Some(&f), "10.11.12.13").unwrap(), "wibble");
        assert_eq!(resolve_one(Some(&f), "10.11.0.13").unwrap(), "wibble");
        assert_eq!(resolve_one(Some(&f), "10.11.256.13").unwrap(), "wibble"); // yes I know this isn't a real IP address
        assert!(resolve_one(Some(&f), "10.11.0.130").is_none());

        assert_eq!(resolve_one(Some(&f), "fred").unwrap(), "barney");
        assert_eq!(resolve_one(Some(&f), "frid").unwrap(), "barney");
        assert!(resolve_one(Some(&f), "freed").is_none());
        assert!(resolve_one(Some(&f), "fredd").is_none());
    }
}
