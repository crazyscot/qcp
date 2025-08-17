#![allow(missing_docs)]

use cfg_aliases::cfg_aliases;

fn main() {
    process_version_string();
    cfg_aliases! {
        linux: { target_os = "linux" },
        bsdish: { any(
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "macos"
        )},
        // This alias is used in the os abstraction layer
        windows_or_dev: { any(
            target_os = "windows",
            debug_assertions
        ) },
        mingw: { all(
            target_os = "windows",
            target_env = "gnu"
        ) },
        msvc: { target_env = "msvc" },
    }

    // Tricky!
    // In a build script, actually evaluating cfg attribs (not via cfg_aliases) gives
    // answers relating to the compiling _host_.
    //
    // This is normally considered harmful, as it is misleading.
    // https://github.com/rust-lang/rust-clippy/issues/9419 refers.
    //
    // However we can use this property to detect, at compile time, whether
    // we're cross compiling for an awkward combination and need to modify our test suite.
    //
    // Also note that you cannot use previously-defined cfg_aliases in a later cfg_aliases block.
    if cfg!(all(target_os = "windows", target_env = "gnu")) {
        // We are building in a mingw environment, so do NOT set cfg alias cross_target_mingw
    } else {
        // We are not building in a mingw environment
        cfg_aliases! {
            cross_target_mingw: { all (target_os = "windows", target_env = "gnu") }
        }
    }
    //dump_build_env();
}

#[allow(dead_code)] // Used for debugging config issues
fn dump_build_env() {
    for (key, value) in std::env::vars() {
        if key.starts_with("CARGO_CFG_") {
            println!("{key}: {value:?}");
        }
    }
    // This panic! is used to ensure Cargo prints the output
    // of the build script to the console.
    panic!("build script output above");
}

fn process_version_string() {
    // trap: docs.rs builds don't get a git short hash
    let hash = git_short_hash().unwrap_or("unknown".into());
    println!("cargo:rustc-env=QCP_BUILD_GIT_HASH={hash}");
    let cargo_version = env!("CARGO_PKG_VERSION");

    let version_string = if let Some(tag) = github_tag() {
        // This is a tagged build running in CI
        println!("cargo:rustc-env=QCP_CI_TAG_VERSION={tag}");
        // Sanity check. We tag releases as "v1.2.3", so strip off the leading v before matching.
        let short_tag = tag.strip_prefix("v").unwrap_or(&tag);
        if cargo_version != short_tag {
            println!(
                "cargo::error=mismatched version tags: cargo={cargo_version}, CI tag={short_tag}"
            );
        }
        tag
    } else {
        format!("{cargo_version}+g{hash}")
    };
    println!("cargo:rustc-env=QCP_VERSION_STRING={version_string}");
}

fn github_tag() -> Option<String> {
    std::env::var("GITHUB_REF_TYPE")
        .is_ok_and(|v| v == "tag")
        .then(|| std::env::var("GITHUB_REF_NAME").unwrap())
}

fn git_short_hash() -> Option<String> {
    use std::process::Command;
    let args = &["rev-parse", "--short=8", "HEAD"];
    if let Ok(output) = Command::new("git").args(args).output() {
        let rev = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if rev.is_empty() { None } else { Some(rev) }
    } else {
        None
    }
}
