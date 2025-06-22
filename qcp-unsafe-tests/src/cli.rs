// Tests which exercise the main modes, but only check exit codes.
// (See the tests in cli_main.rs for reasoning about informational output.)
// (c) 2025 Ross Younger

use std::process::ExitCode;

use rusty_fork::rusty_fork_test;

use qcp::main as qcp_main;

// SAFETY: This sets an env var. Use within rusty_fork_test.
fn main_test(args: &[&str]) -> ExitCode {
    unsafe {
        std::env::set_var("PAGER", "");
    }
    qcp_main(args)
}

rusty_fork_test! {
#[test]
fn help() {
    assert_eq!(main_test(&["qcp", "--help"]), ExitCode::SUCCESS);
}

#[test]
fn help_buffers() {
    assert_eq!(main_test(&["qcp", "--help-buffers"]), ExitCode::SUCCESS);
}
#[test]
fn list_features() {
    assert_eq!(main_test(&["qcp", "--list-features"]), ExitCode::SUCCESS);
}

#[test]
fn show_config() {
    assert_eq!(main_test(&["qcp", "--show-config"]), ExitCode::SUCCESS);
}
#[test]
fn show_config_files() {
    assert_eq!(main_test(&["qcp", "--config-files"]), ExitCode::SUCCESS);
}

#[test]
fn client_no_files() {
    assert_eq!(main_test(&["qcp"]), ExitCode::FAILURE);
}
#[test]
fn server_eof() {
    // We're not setting up stdin (it inherits a closed fd, so gets an EOF)
    assert_eq!(main_test(&["qcp", "--server"]), ExitCode::FAILURE);
}

#[test]
fn bad_option() {
    assert_eq!(main_test(&["qcp", "--this-ridiculous-option-does-not-exist"]), ExitCode::FAILURE);
}

}
