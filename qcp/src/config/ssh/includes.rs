//! Include directive logic
// (c) 2024 Ross Younger

use anyhow::{Context, Result};
use glob::{MatchOptions, glob_with};
use std::path::{MAIN_SEPARATOR, PathBuf};
use std::sync::LazyLock;

static HOME_PREFIX: LazyLock<String> = LazyLock::new(|| format!("~{MAIN_SEPARATOR}"));

fn expand_home_directory(path: &str) -> Result<PathBuf> {
    Ok(match path {
        // bare "~"
        "~" => homedir::my_home()?
            .ok_or_else(|| anyhow::anyhow!("could not determine home directory"))?,
        s if s.starts_with(&*HOME_PREFIX) => {
            // "~/..."
            let Ok(Some(home)) = homedir::my_home() else {
                anyhow::bail!("could not determine home directory")
            };
            home.join(&s[2..])
        }
        s if s.starts_with('~') => {
            // "~someuser/..."
            let mut parts = s[1..].splitn(2, MAIN_SEPARATOR);
            let Some(username) = parts.next() else {
                anyhow::bail!("could not extract username from path")
            };
            let pb = homedir::home(username)?
                .ok_or_else(|| anyhow::anyhow!("could not determine other home directory"))?;
            if let Some(path) = parts.next() {
                pb.join(path)
            } else {
                pb
            }
        }
        // default: no modification
        s => PathBuf::from(s),
    })
}

/// Wildcard matching and ~ expansion for Include directives
pub fn find_include_files(arg: &str, is_user: bool) -> Result<Vec<String>> {
    let mut path = if arg.starts_with('~') {
        anyhow::ensure!(
            is_user,
            "include paths may not start with ~ in a system configuration file"
        );
        expand_home_directory(arg).with_context(|| format!("expanding include expression {arg}"))?
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
#[cfg_attr(coverage_nightly, coverage(off))]
mod test {
    use super::{expand_home_directory, find_include_files};
    use pretty_assertions::assert_eq;

    // Some tests for this module are in `qcp_unsafe_tests::ssh_includes`.

    // helper macro to make the test cases easier to read and write
    macro_rules! xhd {
        ($s:expr) => {
            *expand_home_directory($s).unwrap().as_os_str()
        };
    }

    #[test]
    fn home_dir() {
        let home_env = std::env::var("HOME").unwrap_or("dummy-test-home".into());
        assert_eq!(xhd!("~"), *home_env);

        let s = format!("{home_env}/file");
        assert_eq!(xhd!("~/file"), *s);

        // tricky case. a username.
        let user = std::env::var("USER").expect("this test requires a USER");
        assert!(!user.is_empty());
        let home = dirs::home_dir().expect("this test requires a HOME");
        let path_part = format!("~{user}");
        assert_eq!(xhd!(&path_part), home);

        let s = "any/old~file/";
        assert_eq!(xhd!("any/old~file/"), *s);
    }

    #[test]
    fn include_paths() {
        let _ = find_include_files("~", false).expect_err("~ in system should be disallowed");
        let _ = dirs::home_dir().expect("this test requires a HOME");
        let d = find_include_files("~/zzznonexistent", true)
            .expect("home directory should have expanded");
        assert!(d.is_empty());
        let _ = find_include_files("~nonexistent-user-xyzy", true)
            .expect_err("non existent user should have bailed");
    }
    #[test]
    fn relative_paths() {
        let d = find_include_files("nonexistent-really----", false).expect("");
        assert!(d.is_empty());
        let d = find_include_files("nonexistent-really----", true).expect("");
        assert!(d.is_empty());
    }
}
