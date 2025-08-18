//! Manual page generation
// (c) 2025 Ross Younger

use anyhow::Result;
use pico_args::Arguments;
use std::path::PathBuf;
use xshell::{Shell, cmd};

use crate::top_level;

pub(crate) fn manpage(mut args: Arguments) -> Result<()> {
    let outdir: Option<String> = args.opt_value_from_str(["-o", "--output-directory"])?;
    let release: bool = args.contains(["-r", "--release"]);
    let profile: Option<String> = args.opt_value_from_str(["-p", "--profile"])?;
    if args.contains(["-h", "--help"]) {
        println!(
            "Usage: cargo xtask man [-o|--output-directory DIR] [-r|--release] [-p|--profile PROFILE]"
        );
        return Ok(());
    }
    crate::ensure_all_args_used(args)?;

    let outdir = if let Some(out) = outdir {
        out
    } else {
        // default: <topdir>/qcp/misc
        let pb = PathBuf::from(top_level()?).join("qcp").join("misc");
        pb.to_str().unwrap().to_string()
    };

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());

    let sh = Shell::new()?;
    let mut cmd = cmd!(sh, "{cargo} test");
    if release {
        cmd = cmd.arg("-r");
    }
    if let Some(profile) = profile {
        cmd = cmd.args(["--profile", &profile]);
    }
    cmd.arg("cli::manpage::test::manpages")
        .env("QCP_MANPAGE_OUT_DIR", outdir.clone())
        .run()?;
    println!("Man pages written to {outdir}/");
    Ok(())
}

pub(crate) fn cli_doc(mut args: Arguments) -> Result<()> {
    let outdir: Option<String> = args.opt_value_from_str(["-o", "--output-directory"])?;
    if args.contains(["-h", "--help"]) {
        println!("Usage: cargo xtask clidoc [-o|--output-directory DIR]");
        return Ok(());
    }
    crate::ensure_all_args_used(args)?;

    let outdir = if let Some(out) = outdir {
        out
    } else {
        // default: <topdir>/qcp/misc
        let pb = PathBuf::from(top_level()?).join("qcp").join("misc");
        pb.to_str().unwrap().to_string()
    };

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let sh = Shell::new()?;
    let cmd = cmd!(sh, "{cargo} test");
    cmd.arg("cli::manpage::test::markdown")
        .env("QCP_MANPAGE_OUT_DIR", outdir.clone())
        .run()?;
    println!("Markdown written to {outdir}/qcp.md");

    let cmd = cmd!(sh, "markdown {outdir}/qcp.md").output()?;
    let html = String::from_utf8_lossy(&cmd.stdout);
    std::fs::write(format!("{outdir}/qcp.html"), html.to_string())?;
    println!("HTML written to {outdir}/qcp.html");
    Ok(())
}
