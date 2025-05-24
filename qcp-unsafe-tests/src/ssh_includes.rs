// (c) 2025 Ross Younger

use std::{fs::File, io::Write as _, path::PathBuf};

use rusty_fork::rusty_fork_test;
use tempfile::TempDir;

use qcp::config::find_include_files;

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
