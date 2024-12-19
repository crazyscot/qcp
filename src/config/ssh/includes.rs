//! Include directive logic
// (c) 2024 Ross Younger

use anyhow::{Context, Result};
use glob::{glob_with, MatchOptions};
use std::path::PathBuf;

/// Wildcard matching and ~ expansion for Include directives
pub(super) fn find_include_files(arg: &str, is_user: bool) -> Result<Vec<String>> {
    let mut path = if arg.starts_with('~') {
        anyhow::ensure!(
            is_user,
            "include paths may not start with ~ in a system configuration file"
        );
        expanduser::expanduser(arg)
            .with_context(|| format!("expanding include expression {arg}"))?
    } else {
        PathBuf::from(arg)
    };
    if !path.is_absolute() {
        if is_user {
            let Some(home) = dirs::home_dir() else {
                anyhow::bail!("could not determine home directory");
            };
            let mut buf = home;
            buf.push(".ssh");
            buf.push(path);
            path = buf;
        } else {
            let mut buf = PathBuf::from("/etc/ssh/");
            buf.push(path);
            path = buf;
        }
    }

    let mut result = Vec::new();
    let options = MatchOptions {
        case_sensitive: true,
        require_literal_leading_dot: true,
        require_literal_separator: true,
    };
    for entry in (glob_with(path.to_string_lossy().as_ref(), options)?).flatten() {
        if let Some(s) = entry.to_str() {
            result.push(s.into());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod test {
    use super::find_include_files;

    #[test]
    #[ignore] // this test is dependent on the current user filespace
    fn tilde_expansion_current_user() {
        let a = find_include_files("~/*.conf", true).expect("~ should expand to home directory");
        assert!(!a.is_empty());
        let _ = find_include_files("~/*", false)
            .expect_err("~ should not be allowed in system configurations");
    }

    #[test]
    #[ignore] // obviously this won't run on CI. TODO: figure out a way to make it CIable.
    fn tilde_expansion_arbitrary_user() {
        let a =
            find_include_files("~wry/*.conf", true).expect("~ should expand to a home directory");
        println!("{a:?}");
        assert!(!a.is_empty());
        let _ = find_include_files("~/*", false)
            .expect_err("~ should not be allowed in system configurations");
    }

    #[test]
    #[ignore] // TODO: Make this runnable on CI
    fn relative_path_expansion() {
        let a = find_include_files("config", true).unwrap();
        println!("{a:?}");
        assert!(!a.is_empty());

        let a = find_include_files("sshd_config", false).unwrap();
        println!("{a:?}");
        assert!(!a.is_empty());

        // but the user does not have an sshd_config:
        let a = find_include_files("sshd_config", true).unwrap();
        println!("{a:?}");
        assert!(a.is_empty());
    }
}
