// (c) 2025 Ross Younger

use qcp::Configuration;
use qcp::config::Manager;

use littertray::LitterTray;
use rusty_fork::rusty_fork_test;

unsafe fn prepare_fake_home(tray: &LitterTray) {
    // Temporarily override HOME environment variable
    // (this must only happen in single-threaded code)
    let fake_home = dunce::canonicalize(tray.directory()).unwrap();
    #[allow(unsafe_code)]
    unsafe {
        // Unix only! This is not sufficient on Windows.
        std::env::set_var("HOME", &fake_home);
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
    // TODO: Windows config file logic uses the Known Folder system via dirs::config_dir(),
    // and I don't know how to mock that up for testing. Setting UserProfile isn't sufficient.
    #[cfg_attr(target_os = "windows", ignore)]
    #[test]
    fn standard_reads_config_with_host() {
        LitterTray::run(|tray| {
            unsafe {
                prepare_fake_home(tray);
            }
            eprintln!("Fake home is {:?}", std::env::var("HOME").unwrap());
            eprintln!("Config file paths are {:?}", Manager::config_files());
            let mut mgr1 = Manager::standard(Some("testhost"));
            mgr1.apply_system_default();
            //eprintln!("mgr1: {mgr1:?}");
            assert_eq!(mgr1.get::<Configuration>().unwrap().rx(), 1234);
            mgr1.validate_configuration().unwrap();
        });
    }

    // TODO: Windows config file logic uses the Known Folder system via dirs::config_dir(),
    // and I don't know how to mock that up for testing. Setting UserProfile isn't sufficient.
    #[cfg_attr(target_os = "windows", ignore)]
    #[test]
    fn standard_reads_config_without_host() {
        LitterTray::run(|tray| {
            unsafe {
                prepare_fake_home(tray);
            }
            let mut mgr1 = Manager::standard(None);
            mgr1.apply_system_default();
            //eprintln!("mgr1: {mgr1:?}");
            assert_eq!(mgr1.get::<Configuration>().unwrap().rx(), 2345);
            mgr1.validate_configuration().unwrap();
        });
    }

}
