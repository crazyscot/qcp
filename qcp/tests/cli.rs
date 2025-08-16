//! CLI based tests
use std::process::ExitCode;

use qcp::main as qcp_main;

use rusty_fork::rusty_fork_test;

#[test]
fn show_config_files() {
    assert_eq!(qcp_main(["qcp", "--config-files"]), ExitCode::SUCCESS);
}

#[test]
fn bad_option() {
    assert_eq!(
        qcp_main(["qcp", "--this-ridiculous-option-does-not-exist"]),
        ExitCode::FAILURE
    );
}

rusty_fork_test! {

#[test]
fn client_no_files() {
    assert_eq!(qcp_main(["qcp"]), ExitCode::FAILURE);
}

#[test]
fn ssh_fails() {
    assert_eq!(
        qcp_main([
            "qcp",
            "-S",
            "-oConnectTimeout=-1",
            "127.0.0.1:nosuchfile",
            "/nosuchfile"
        ]),
        ExitCode::FAILURE
    );
}

}
