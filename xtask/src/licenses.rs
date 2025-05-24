//! License summary generation
// (c) 2025 Ross Younger
use anyhow::{Result, anyhow};
use pico_args::Arguments;
use std::path::PathBuf;
use xshell::{Shell, cmd};

#[derive(Debug)]
struct Args {
    output: Option<String>,
}

pub(crate) fn licenses(mut args: Arguments) -> Result<()> {
    let res = Args {
        output: args.opt_value_from_str(["-o", "--output"])?,
    };
    crate::ensure_all_args_used(args)?;

    let output = match res.output {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(std::env::var_os("OUT_DIR").ok_or(anyhow!("OUT_DIR not set"))?)
            .join("licenses.html"),
    };

    let sh = Shell::new()?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let target_str = sh.var("QCP_BUILD_TARGET").ok();
    let target_opt = if target_str.is_some() {
        Some("--target")
    } else {
        None
    };
    cmd!(
        sh,
        "{cargo} about generate qcp/misc/licenses.hbs -o {output} --fail --locked {target_opt...} {target_str...}"
    )
    .run()?;
    // If about complains about licenses not being harvested, you can ask clearlydefined.io to harvest these - but it's not essential.

    // TODO: currently available in nightly: pathbuf.add_extension(".gz");
    let mut extension = output
        .extension()
        .unwrap_or_default()
        .to_str()
        .unwrap()
        .to_string();
    if !extension.is_empty() {
        // Only push a '.' if there is already an extension (otherwise you get foo..gz)
        extension.push('.');
    }
    extension.push_str("gz");
    let mut output_gz = output.clone();
    output_gz.set_extension(extension);
    crate::gzip(output.clone(), output_gz.clone())?;
    eprintln!("Wrote {} and {}", output.display(), output_gz.display());
    Ok(())
}
