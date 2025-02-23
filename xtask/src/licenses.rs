use anyhow::{anyhow, Result};
use pico_args::Arguments;
use std::path::PathBuf;
use xshell::{cmd, Shell};

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

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let sh = Shell::new()?;
    cmd!(
        sh,
        "{cargo} about generate qcp/misc/licenses.hbs -o {output}"
    )
    .run()?;
    Ok(())
}
