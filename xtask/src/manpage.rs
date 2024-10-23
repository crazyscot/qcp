use anyhow::{anyhow, Result};
use clap::CommandFactory as _;
use pico_args::Arguments;
use roff::{italic, roman, Roff};
use std::io::Write;
use std::path::PathBuf;

#[derive(Debug)]
struct Args {
    output: Option<String>,
}

pub(crate) fn manpage(mut args: Arguments) -> Result<()> {
    let res = Args {
        output: args.opt_value_from_str(["-o", "--output"])?,
    };
    crate::ensure_all_args_used(args)?;

    let output = match res.output {
        Some(p) => PathBuf::from(p),
        None => PathBuf::from(std::env::var_os("OUT_DIR").ok_or(anyhow!("OUT_DIR not set"))?)
            .join("qcp.1"),
    };

    let cmd = qcp::cli::CliArgs::command();
    let man = clap_mangen::Man::new(cmd.clone());
    let mut buffer: Vec<u8> = Default::default();
    man.render_title(&mut buffer)?;
    man.render_name_section(&mut buffer)?;
    man.render_synopsis_section(&mut buffer)?;
    man.render_description_section(&mut buffer)?;
    usage(&cmd, &mut buffer)?;
    man.render_options_section(&mut buffer)?;
    additional(&mut buffer)?;
    man.render_authors_section(&mut buffer)?;

    std::fs::write(&output, buffer)?;
    println!("Wrote to {output:?}");
    Ok(())
}

fn usage(cmd: &clap::Command, w: &mut dyn Write) -> Result<(), std::io::Error> {
    let mut roff = Roff::default();
    roff.control("SH", ["USAGE"]);
    roff.control("TP", []);
    roff.control("BI", ["qcp ", "[OPTIONS] ", "[SOURCE] ", "[DESTINATION]"]);
    if let Some(about) = cmd.get_before_long_help().or_else(|| cmd.get_before_help()) {
        for line in about.to_string().lines() {
            if line.trim().is_empty() {
                roff.control("PP", []);
            } else {
                roff.text([roman(line)]);
            }
        }
    }
    roff.control("SS", ["LIMITATIONS"]);
    roff.control("TP", []);
    roff.text([roman("You must be able to ssh directly to the remote machine, and exchange UDP packets with it on a given port. (If the local machine is behind connection-tracking NAT, things work just fine. This is the case for the vast majority of home and business network connections.)")]);

    roff.control("TP", []);
    roff.text([roman(
        "Be aware that network security systems can’t readily identify QUIC traffic as such. It’s opaque, and high bandwidth. Some security systems might flag it as a potential threat.
",
    )]);

    roff.control("SS", ["CAVEATS"]);
    roff.control("TP", []);
    roff.text([roman(
        "This is an experimental implementation of an experimental protocol. While it has security goals, these have not been verified."
    )]);

    roff.to_writer(w)
}

fn additional(w: &mut dyn Write) -> Result<(), std::io::Error> {
    let mut roff = Roff::default();
    roff.control("SH", ["EXIT STATUS"]);
    roff.control("TP", []);
    roff.text([roman(
        "The qcp utility exits 0 on success, and >0 if an error occurs.",
    )]);

    roff.control("SH", ["NETWORK PROTOCOL"]);
    roff.control("TP", []);
    roff.text([
        roman("qcp is a "),
        italic("hybrid"),
        roman(" protocol. We use "),
        italic("ssh"),
        roman(" to establish a control channel and exchange ephemeral TLS certificates, then a "),
        italic("QUIC"),
        roman(" connection to transport data."),
    ]);

    roff.control("TP", []);
    roff.text([roman("Detailed protocol documentation can be found at")]);
    roff.control("UR", ["https://docs.rs/qcp/latest/qcp/protocol/"]);
    roff.control("UE", []);

    roff.control("SS", ["PERFORMANCE TUNING"]);
    roff.text([roman("See")]);
    roff.control("UR", ["https://docs.rs/qcp/latest/qcp/doc/performance/"]);
    roff.control("UE", []);

    roff.control("SS", ["TROUBLESHOOTING"]);
    roff.text([roman("See")]);
    roff.control(
        "UR",
        ["https://docs.rs/qcp/latest/qcp/doc/troubleshooting/"],
    );
    roff.control("UE", []);

    roff.control("SH", ["BUGS"]);
    roff.control("TP", []);
    roff.text([roman("Please report any you find via the issue tracker: ")]);
    roff.control("UR", ["https://github.com/crazyscot/qcp/issues"]);
    roff.control("UE", []);

    roff.control("SH", ["SEE ALSO"]);
    roff.control("TP", []);
    roff.control(
        "BR",
        [
            "ssh(1), ",
            "ssh_config(5), ",
            "RFC 4254, ",
            "RFC 9000, ",
            "RFC 9001",
        ],
    );

    roff.to_writer(w)
}
