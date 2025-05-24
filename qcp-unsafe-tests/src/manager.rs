// (c) 2025 Ross Younger

use qcp::Configuration;
use qcp::config::Manager;

use littertray::LitterTray;
use rusty_fork::rusty_fork_test;

unsafe fn prepare_fake_home(tray: &LitterTray) {
    // Temporarily override HOME environment variable
    // (this must only happen in single-threaded code)
    let fake_home = tray.directory();
    #[allow(unsafe_code)]
    unsafe {
        std::env::set_var("HOME", fake_home);
    };

    tray.create_text(
        ".qcp.conf",
        r"
                Host testhost
                rx 1234
                Host *
                rx 2345
            ",
    )
    .unwrap();
}

rusty_fork_test! {
    #[test]
    fn standard_reads_config_with_host() {
        LitterTray::run(|tray| {
            unsafe {
                prepare_fake_home(tray);
            }
            let mut mgr1 = Manager::standard(Some("testhost"));
            mgr1.apply_system_default();
            //eprintln!("mgr1: {mgr1:?}");
            assert!(mgr1.get::<Configuration>().unwrap().rx == 1234u64.into());
            mgr1.validate_configuration().unwrap();
        });
    }

    #[test]
    fn standard_reads_config_without_host() {
        LitterTray::run(|tray| {
            unsafe {
                prepare_fake_home(tray);
            }
            let mut mgr1 = Manager::standard(None);
            mgr1.apply_system_default();
            //eprintln!("mgr1: {mgr1:?}");
            assert!(mgr1.get::<Configuration>().unwrap().rx == 2345u64.into());
            mgr1.validate_configuration().unwrap();
        });
    }

}
