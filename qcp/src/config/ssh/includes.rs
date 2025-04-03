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
pub(super) fn find_include_files(arg: &str, is_user: bool) -> Result<Vec<String>> {
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
    use std::{fs::File, io::Write as _, path::PathBuf};

    use super::{expand_home_directory, find_include_files};
    use rusty_fork::rusty_fork_test;
    use tempfile::TempDir;

    fn fake_home_ssh() -> (TempDir, PathBuf) {
        let tempdir = tempfile::tempdir().unwrap();
        let fake_home = tempdir.path();
        let fake_ssh = fake_home.join(".ssh");
        std::fs::create_dir_all(&fake_ssh).unwrap();

        // Temporarily override HOME environment variable
        // (this must only happen in single-threaded code)
        #[allow(unsafe_code)]
        unsafe {
            std::env::set_var("HOME", fake_home);
        };

        (tempdir, fake_ssh)
    }

    // We run some tests in a fork because they modify environment variables, which
    // could interfere with other tests.

    rusty_fork_test! {
    #[test]
    fn tilde_expansion_current_user() {
        let (fake_home, fake_ssh) = fake_home_ssh();

        // Create a test .conf file
        let test_conf = fake_ssh.join("test.conf");
        std::fs::write(&test_conf, "dummy content").unwrap();

        // Create a test file within
        let filename = format!("{}/foo.conf", fake_home.path().display());
        let mut file = File::create(&filename).unwrap();
        file.write_all(b"Hello, world!").unwrap();

        let a = find_include_files("~/*.conf", true).expect("~ should expand to home directory");
        assert_eq!(a, [filename]);
        let _ = find_include_files("~/*", false)
            .expect_err("~ should not be allowed in system configurations");
    }}

    rusty_fork_test! {
    #[test]
    fn relative_path_expansion() {
        // relative expansion in ~/.ssh/
        let (_fake_home, fake_ssh) = fake_home_ssh();
        let filename = format!("{}/my_config", fake_ssh.display());
        println!("{filename:?}");
        let mut file = File::create(&filename).unwrap();
        file.write_all(b"Hello, world!").unwrap();
        let a = find_include_files("my_config", true).unwrap();
        assert_eq!(a, [filename]);
        let a = find_include_files("nonexistent_config", true).unwrap();
        assert!(a.is_empty());

        // we haven't yet figured out a way to test the contents of /etc/ssh, but we can at least perform a negative test
        let a = find_include_files("zyxy_nonexistent_file", false).unwrap();
        assert!(a.is_empty());
    }}

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
