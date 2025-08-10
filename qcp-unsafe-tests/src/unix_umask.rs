use std::fs::Permissions;
use std::os::unix::fs::PermissionsExt as _;

use littertray::LitterTray;
use rusty_fork::rusty_fork_test;

#[tokio::main(flavor = "current_thread")]
async fn test_umask(initial: u16, set_umask: u16) {
    use std::fs::set_permissions;

    let expected = initial & !set_umask;

    let src = "src";
    let dest = "dest";
    let rdest = "remote:dest";
    unsafe {
        // umask takes a u32 on Linux but a u16 on macos
        libc::umask(set_umask.into());
    }

    LitterTray::try_with_async(async |tray| {
        let _ = tray.create_text(src, "hi")?;
        set_permissions(src, Permissions::from_mode(initial.into()))?;

        let (r1, r2) = qcp::test_helpers::test_getx_main(src, rdest, 2, 2, false).await?;
        assert!(r1.is_ok());
        assert!(r2.is_ok());

        let meta = std::fs::metadata(dest)?;
        let result = meta.permissions().mode() & 0o777;
        assert_eq!(
            result,
            expected.into(),
            "result file mode was {result:0>3o} but expected {expected:0>3o}"
        );
        Ok(())
    })
    .await
    .unwrap();
}

rusty_fork_test! {
// rusty_fork_test doesn't currently support #[tokio::test], so we have to indirect.
#[test]
fn get_umask_modes() {
    for mode in [0o777, 0o666, 0o555, 0o444] {
        for mask in [0o022, 0o002, 0o000] {
            test_umask(mode, mask);
        }
    }
}
}
