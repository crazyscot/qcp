// (c) 2025 Ross Younger

use qcp::Configuration;
use qcp::config::Manager;

use rusty_fork::rusty_fork_test;
use tempfile::TempDir;

#[allow(unsafe_code)]
pub(crate) unsafe fn fake_home() -> TempDir {
    let tempdir = tempfile::tempdir().unwrap();
    let fake_home = tempdir.path();

    // Temporarily override HOME environment variable
    // (this must only happen in single-threaded code)
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("HOME", fake_home);
    };
    tempdir
}

rusty_fork_test! {
    #[test]
    fn standard_reads_config() {
        #[allow(unsafe_code)]
        let home = unsafe {
            // SAFETY: this is run in a rusty-fork test, so it won't affect other tests
            fake_home()
        };
        let test_conf = home.path().join(".qcp.conf");
        let files = Manager::config_files();
        assert!(files.contains(&test_conf.to_string_lossy().to_string()));
        std::fs::write(&test_conf,
            r"
                Host testhost
                rx 1234
                Host *
                rx 2345
            ",
        ).unwrap();
        let mut mgr1 = Manager::standard(Some("testhost"));
        mgr1.apply_system_default();
        eprintln!("mgr1: {mgr1:?}");
        assert!(mgr1.get::<Configuration>().unwrap().rx == 1234u64.into());
        mgr1.validate_configuration().unwrap();

        let mut mgr2 = Manager::standard(None);
        mgr2.apply_system_default();
        mgr2.validate_configuration().unwrap();
        assert!(mgr2.get::<Configuration>().unwrap().rx == 2345u64.into());
    }
}
