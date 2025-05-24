//! xtask to create dummy debian changelog for this package
// (c) 2025 Ross Younger
use std::{fs::File, io::Write, path::PathBuf};

use anyhow::Result;
use cargo_toml::Manifest;
use pico_args::Arguments;

static DEBEMAIL: &str = "qcp@crazyscot.com";
static DEBFULLNAME: &str = "QCP Team";
static DISTRO: &str = "generic";

pub(crate) fn changelog(mut args: Arguments) -> Result<()> {
    let package: String = args.value_from_str(["-p", "--package"])?;
    crate::ensure_all_args_used(args)?;

    // Get package cargo version
    let path = PathBuf::from(package.clone()).join("Cargo.toml");
    let m = Manifest::from_path(path)?;
    let version = m.package().version();

    // 2. Traditionally the developer would invoke the `dch' script (in devscripts).
    //    But we know exactly what this file has to contain, so we'll cut to the chase.

    let outpath = PathBuf::from(package.clone())
        .join("debian")
        .join("changelog");
    let mut outfile = File::create(outpath)?;
    let date = chrono::Utc::now().to_rfc2822();
    write!(
        outfile,
        r"{package} ({version}) {DISTRO}; urgency=medium

  * New upstream release.
    See /usr/share/doc/{package}/changelog.gz for full details.

 -- {DEBFULLNAME} <{DEBEMAIL}>  {date}
 "
    )?;
    outfile.flush()?;
    Ok(())
}

// .. add to gitignore
// .. test build deb pkg, is it included correctly?
