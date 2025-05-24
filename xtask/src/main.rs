//! xtask?
//! See <https://github.com/matklad/cargo-xtask>
// (c) 2025 Ross Younger

use std::{fs::File, io::BufReader, path::PathBuf, process::Command};

use anyhow::{Context as _, Result};
use flate2::{Compression, GzBuilder};
use pico_args::Arguments;

mod dch;
mod licenses;
mod manpage;

// ---------------------------------------------------------------------------------------------
// Task definition
//
// Syntax: (Command-line verb, implementing function, description for help message)

#[allow(clippy::type_complexity)]
const TASKS: &[(&str, fn(Arguments) -> Result<()>, &str)] = &[
    ("man", manpage::manpage, "Build the qcp manual pages"),
    (
        "licenses",
        licenses::licenses,
        "Generate licenses.html  (prerequisite: `cargo install about`)",
    ),
    ("dch", dch::changelog, "Generate the dummy debian changelog"),
    ("help", help, "Output help"),
];

// ---------------------------------------------------------------------------------------------

fn main() {
    if let Err(e) = main_guts() {
        eprintln!("Error: {e:?}");
        std::process::exit(1);
    }
}

fn help(_: Arguments) -> Result<()> {
    println!("Supported tasks:");
    let longest = TASKS
        .iter()
        .fold(0, |acc, (verb, _, _)| std::cmp::max(acc, verb.len()));
    let mut display: Vec<_> = TASKS.iter().collect();
    display.sort_by_key(|(verb, _, _)| *verb);
    for (verb, _, msg) in display {
        println!("  {verb:0$}  {msg}", longest);
    }
    Ok(())
}

fn main_guts() -> Result<()> {
    ensure_top_level()?;
    let mut args = Arguments::from_env();
    let cmd = args.subcommand()?;
    if let Some(task) = cmd.as_deref() {
        TASKS
            .iter()
            .find_map(|(verb, fun, _)| (*verb == task).then_some(*fun))
            .unwrap_or(help)/*it's a function, call it!*/(args)
    } else {
        help(args)
    }?;
    Ok(())
}

fn top_level() -> Result<String> {
    let toplevel_path = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Invoking git rev-parse")?;
    if !toplevel_path.status.success() {
        anyhow::bail!("Failed to invoke git rev-parse");
    }
    Ok(String::from_utf8(
        toplevel_path.stdout.trim_ascii().to_vec(),
    )?)
}

fn ensure_top_level() -> Result<()> {
    std::env::set_current_dir(top_level()?).context("Changing to toplevel")?;
    Ok(())
}

pub(crate) fn ensure_all_args_used(args: Arguments) -> Result<()> {
    let unused = args.finish();
    anyhow::ensure!(
        unused.is_empty(),
        format!("Unhandled arguments: {unused:?}"),
    );
    Ok(())
}

/// This is essentially the `gzip` shell command
fn gzip(from: PathBuf, to: PathBuf) -> Result<()> {
    let filename = from.clone();
    let filename = filename.file_name().unwrap().to_str().unwrap();
    let infile = File::open(from)?;
    let mut inbuffer = BufReader::new(infile);
    let outfile = File::create(to)?;
    let mut gz = GzBuilder::new()
        .filename(filename)
        .write(outfile, Compression::default());
    std::io::copy(&mut inbuffer, &mut gz)?;
    let _ = gz.finish()?;
    Ok(())
}
