/// Quasi-test that generates the qcp.1 manpage
///
/// This is mildly unorthodox.
/// Why on earth have this as a test, you might ask?
///
/// Rationale:
/// - xtasks should not depend on building the entire package
/// - tests already do depend on qcp, so it's pretty cheap in terms of build time to do this here
/// - either way, the mangen/roff machinery does not bloat the main binary
/// - buildability of the man page is in fact a meaningful test
#[cfg_attr(coverage_nightly, coverage(off))]
#[cfg(test)]
mod test {

    #![allow(unused_results)]

    use crate::Configuration;
    use crate::cli::args::CliArgs;

    use anyhow::{Result, anyhow};
    use clap::CommandFactory as _;
    use roff::{Roff, italic, roman};
    use std::io::Write;
    use std::path::PathBuf;
    use struct_field_names_as_array::FieldNamesAsSlice;

    #[test]
    fn manpages() -> Result<()> {
        man1()?;
        man5()
    }

    fn man1() -> Result<()> {
        let output = PathBuf::from(
            std::env::var_os("QCP_MANPAGE_OUT_DIR")
                .or_else(|| std::env::var_os("OUT_DIR"))
                .ok_or(anyhow!("Neither QCP_MANPAGE_OUT_DIR nor OUT_DIR is set"))?,
        )
        .join("qcp.1");

        let cmd = CliArgs::command();
        let man = clap_mangen::Man::new(cmd.clone());
        let mut buffer: Vec<u8> = Vec::default();
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

    fn usage<W: Write>(cmd: &clap::Command, w: &mut W) -> Result<(), std::io::Error> {
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

    fn additional<W: Write>(w: &mut W) -> Result<(), std::io::Error> {
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
            roman(
                " to establish a control channel and exchange ephemeral TLS certificates, then a ",
            ),
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

    #[allow(clippy::too_many_lines)]
    fn man5() -> Result<()> {
        let output = PathBuf::from(
            std::env::var_os("QCP_MANPAGE_OUT_DIR")
                .or_else(|| std::env::var_os("OUT_DIR"))
                .ok_or(anyhow!("Neither QCP_MANPAGE_OUT_DIR nor OUT_DIR is set"))?,
        )
        .join("qcp_config.5");

        let mut page = Vec::<u8>::new();
        page.extend_from_slice(MAN5_PART1.as_bytes());

        let fields = Configuration::FIELD_NAMES_AS_SLICE
            .iter()
            .fold(Vec::new(), |mut acc, it| {
                if !acc.is_empty() {
                    acc.push(roman(", "));
                }
                acc.push(italic(*it));
                acc
            });
        Roff::default().text(fields).to_writer(&mut page)?;
        //page.push_str(&Roff::default().text(fields).render());
        page.extend_from_slice(MAN5_PART2.as_bytes());

        std::fs::write(&output, page)?;
        println!("Wrote to {output:?}");
        Ok(())
    }

    static MAN5_PART1: &str = r#"
.ie \n(.g .ds Aq \(aq
.el .ds Aq '
.TH QCP_CONFIG 5
.SH NAME
qcp_config \- Configuration options for qcp

.SH DESCRIPTION
\fBqcp\fR(1) obtains run-time configuration from the following sources, in order:

.RS 0
.IP
1. Command-line options
.IP
2. The user's configuration file (usually \fI~/.qcp.conf\fR) on either side of the connection (see NEGOTIATION)
.IP
3. The system-wide configuration file (usually \fI/etc/qcp.conf\fR) on either side of the connection
.IP
4. Hard-wired defaults
.RE

Each option may appear in multiple places, but only the first match is used.

\fBNote:\fR Configuration file locations are platform-dependent. To see what applies on the current platform, run \fIqcp --config-files\fR.

.SH FILE FORMAT

qcp uses the same basic format as OpenSSH configuration files.

Each line contains a keyword (the option name) and one or more arguments.

Option names are single words. They are case insensitive; hyphens and underscores are ignored.
In other words, you can use UPPERCASE, lowercase, CamelCase, mIxEDcAse, SHOUTY_SNAKE_CASE, kebab-case, Train-Case, whatever you like.

Arguments are separated from keywords, and each other, by whitespace.
(It is also possible to write \fIKey=Value\fR or \fIKey = Value\fR.)

Arguments may be surrounded by double quotes ("); this allows you to set an argument containing spaces.
If a backslash, double or single quote forms part of an argument it must be backslash-escaped i.e. \\" or \\\\

Comments are supported; # introduces a comment (up to the end of the line).

Empty lines are ignored.

\fIqcp supports Host and Include directives in way that is intended to be compatible with OpenSSH.\fR
This allows you to tune your configuration for a range of network hosts.

.SH EXAMPLE
 Host old-faithful
 # This is an old server with a very limited CPU which we do not want to overstress.
 # old-faithful isn't its DNS name; it's a hostname aliased in ssh_config.
 rx 125k  # 1 Mbit limit. Yes, it's a really old server.
 tx 0     # tx 0 means \"same as rx\"
 # This server runs a tight firewall; inbound UDP is only allowed on certain ports.
 RemotePort 65400-65500
 
 Host *.internal.corp 172.31.200.*
 # This is a nearby data centre which we have a dedicated 1Gbit connection to.
 # We don't need to use qcp, but it's convenient to use one tool in our scripts.
 # We specify the group both by domain name and netblock: the qcp client process
 # matches against whatever you give on the command line, and the qcp server
 # uses only the connecting IP address.
 # (IPv6 addresses would work here too.)
 rx 125M
 tx 0
 rtt 10
 
 # For all other hosts, try to maximise our 400Mbit fibre
 Host *
 rx 50M         # we have 400Mbit download
 tx 12.5M       # we have 100Mbit upload; we could also have written this out in full, or as \"12M5\"
 rtt 150        # most servers we care about are an ocean away
 congestion bbr # this works well for us

.SH CONFIGURATION DIRECTIVES

.TP
\fBHost\fR \fIpattern [pattern ...]\fR
Introduces a \fIhost block\fR.
All following options - up to the next Host - only apply to hosts matching any of the patterns given.

Pattern matching uses '*' and '?' as wildcards in the usual way.

A single asterisk '*' matches all hosts; this is used to provide defaults.

A pattern beginning with '!' is a \fInegative\fR match; it matches all remote hosts \fIexcept\fR those matching the rest of the pattern.

Pattern matching is applied directly to the remote host given on the QCP command line, before DNS or alias resolution.
\fIIt does _not_ resolve hostname to IP address.\fR
However, if you connect to hosts by IP address, patterns (for example \fI10.11.12.*\fR) do work in the obvious way.

The remote (server) process reads its own configuration file and matches using the IP address passed to it by the ssh server in the \fISSH_CONNECTION\fR or \fISSH_CLIENT\fR environment variables.

.TP
\fBInclude\fR \fIfile [file ...]\fR

Include the specified file(s) in the configuration at the current point. Glob wildcards ('*' and '?') are supported in filenames.

User configuration files may refer to pathnames relative to '~' (the user's home directory).

Filenames with relative paths are assumed to be in \fI~/.ssh/\fR if read from a user configuration file, or \fI/etc/ssh/\fR if read from a system configuration file.

An Include directive inside a Host block retains the Host context.
This may be useful to apply common directives to multiple hosts with minimal repetition.
Note that if an included file begins a new Host block, that will continue to apply on return to the including file.

It is possible for included files to themselves include additional files; there is a brake that prevents infinite recursion.

.SH CONFIGURABLE OPTIONS

The following options from the CLI are supported in configuration files:
"#;

    // insert the autogenerated options list here //

    static MAN5_PART2: &str = r"
Refer to \fBqcp\fR(1) for details.

In configuration files, option keywords are case insensitive and ignore hyphens and underscores.
(On the command line, options must be specified in kebab-case.)
For example, these declarations are all equivalent:
    RemotePort 12345
    remoteport 12345
    remote_port 12345
    Remote_Port 12345
    ReMoTePoRt 12345
    rEmOtE-pOrT 12345

.SH CONFIGURATION DEBUG TOOLS

The \fI--dry-run\fR mode reports the negotiated network configuration that we would have used for a given proposed transfer.

.IP
2025-02-10 09:32:07Z  INFO Negotiated network configuration: rx 37.5MB (300Mbit), tx 12.5MB (100Mbit), rtt 300ms, congestion algorithm Cubic with initial window <default>
.RE

As configurations can get quite complex, it may be useful to understand where a particular value came from.

qcp will do this for you, with the \fI--show-config\fR option.
Specify a source and destination as if you were copying a file to/from a host to see the configuration that would apply. For example:

.IP
 $ qcp --show-config server234:some-file /tmp/

 ┌─────────────────────────┬─────────────┬───────────────────────────────┐
 │ field                   │ value       │ source                        │
 ├─────────────────────────┼─────────────┼───────────────────────────────┤
 │ (Remote host)           │ server234   │                               │
 │ AddressFamily           │ any         │ default                       │
 │ Congestion              │ Cubic       │ default                       │
 │ InitialCongestionWindow │ 0           │ default                       │
 │ Port                    │ 10000-12000 │ /home/xyz/.qcp.conf (line 26) │
 │ RemotePort              │ 0           │ default                       │
 │ Rtt                     │ 300         │ default                       │
 │ Rx                      │ 37M5        │ /home/xyz/.qcp.conf (line 22) │
 │ Ssh                     │ ssh         │ default                       │
 │ SshConfig               │ []          │ default                       │
 │ SshOptions              │ []          │ default                       │
 │ TimeFormat              │ UTC         │ /etc/qcp.conf (line 14)       │
 │ Timeout                 │ 5           │ default                       │
 │ Tx                      │ 12M5        │ /home/xyz/.qcp.conf (line 23) │
 └─────────────────────────┴─────────────┴───────────────────────────────┘
.RE

Add `--remote-config` to the command line to have the server report its settings.
These come in two blocks, the \fIstatic\fR configuration and the \fIfinal\fR configuration after applying system defaults and client preferences.
.IP
 % qcp --remote-config server234:some-file /tmp/
 2025-02-10 09:26:02Z  INFO Server: Static configuration:
 ┌───────────────┬─────────────┬───────────────────────────────┐
 │ field         │ value       │ source                        │
 ├───────────────┼─────────────┼───────────────────────────────┤
 │ (Remote host) │ 10.22.55.77 │                               │
 │ Port          │ 10000-12000 │ /home/xyz/.qcp.conf (line 26) │
 │ Rtt           │ 1           │ /home/xyz/.qcp.conf (line 19) │
 │ Rx            │ 125M        │ /home/xyz/.qcp.conf (line 17) │
 │ TimeFormat    │ UTC         │ /home/xyz/.qcp.conf (line 25) │
 │ Tx            │ 125M        │ /home/xyz/.qcp.conf (line 18) │
 └───────────────┴─────────────┴───────────────────────────────┘
 2025-02-10 09:26:02Z  INFO Server: Final configuration:
 ┌─────────────────────────┬─────────────┬───────────────────────────────┐
 │ field                   │ value       │ source                        │
 ├─────────────────────────┼─────────────┼───────────────────────────────┤
 │ (Remote host)           │ 10.22.55.77 │                               │
 │ AddressFamily           │ any         │ default                       │
 │ Congestion              │ Cubic       │ default                       │
 │ InitialCongestionWindow │ 0           │ default                       │
 │ Port                    │ 10000-12000 │ /home/xyz/.qcp.conf (line 26) │
 │ RemotePort              │ 0           │ default                       │
 │ Rtt                     │ 1           │ /home/xyz/.qcp.conf (line 19) │
 │ Rx                      │ 123M        │ config resolution logic       │
 │ Ssh                     │ ssh         │ default                       │
 │ SshConfig               │ []          │ default                       │
 │ SshOptions              │ []          │ default                       │
 │ TimeFormat              │ UTC         │ /home/xyz/.qcp.conf (line 25) │
 │ Timeout                 │ 5           │ default                       │
 │ Tx                      │ 125M        │ /home/xyz/.qcp.conf (line 18) │
 └─────────────────────────┴─────────────┴───────────────────────────────┘
.RE

.SH TIPS AND TRAPS
1. Like OpenSSH, for each setting we use the value from the \fIfirst\fR Host block we find that matches the remote hostname.

2. Each setting is evaluated independently.
In the example above, the \fIHost old-faithful\fR block sets rx but does not set rtt.
Any operations to old-faithful inherit \fIrtt 150\fR from the Host * block.

3. The tx setting has a default value of 0, which means “use the active rx value”.
\fIIf you set tx in a Host * block, you probably want to set it explicitly everywhere you set rx.\fR

4. The qcp client process does \fINOT\fR resolve hostname to IP address when determining which `Host` block(s) to match.
   This is consistent with OpenSSH.
.IP
   * However, the qcp server process ONLY matches against the IP address passed to it by the OpenSSH server.
   * Therefore, \fIin an environment which may act as both qcp client and server, you may need to specify options by both hostname and netblock\fR.
.RE

.SH BUILDING A CONFIGURATION

We suggest the following approach to setting up a configuration file.

   1. Set up a `Host *` block specifying `Tx` and `Rx` suitable for your local network uplink.
.IP
* In a data centre environment, the bandwidth limits will likely be whatever your network interface is capable of.
(If the data centre has limited bandwidth, or your contract specifies something lower, use that instead.)

* In a host connected to a standard ISP connection, the bandwidth limits will be whatever you're paying your ISP for.
If you're not sure, you might use speedtest.net or a similar service.
.RE

2. Make a best-guess to what the Round Trip Time might be in the default case, and add this to `Host *`.
.IP
* If you mostly deal with servers on the same continent as you, this might be somewhere around 50 or 100ms.

* If you mostly deal with servers on the other side of the planet, this might be 300s or even more.
.RE

3. Add any other global options to the `Host *` block
.IP
* If this machine will act as a qcp server and has a firewall that limits incoming UDP traffic, set up a firewall exception on a range of ports and configure that as `port`.

* Set up any non-standard `ssh`, `ssh_options` or `ssh_config` that may be needed.

* If you want to use UTC when printing messages, set `TimeFormat`.
.RE

4. If there are any specific hosts or network blocks that merit different network settings, add `Host` block(s) for them as required.

.IP
* Order these from most-specific to least-specific.
Be sure to place them \fIabove\fR `Host *` in the config file.
.RE

5. Try it out! Copy some files around and see what network performance is like.
\fINote that these files need to be large (hundreds of MB or more) to really see the effect,
and you need to go into gigabytes to see it do well on a good fibre connection.\fR

You might like to use `--dry-run` mode to preview the final network configuration for a proposed file transfer.
If the output isn't what you expected, use `--show-config` and `--remote-config` to see where the various settings came from.
\fINote that `tx' and `rx' are the opposite way round, from from the server's point of view!\fR

.SH FILES

.TP
~/.qcp.conf
The user configuration file (on most platforms)

.TP
/etc/qcp.conf
The system configuration file (on most platforms)

.TP
~/.ssh/ssh_config
The user ssh configuration file

.TP
/etc/ssh/ssh_config
The system ssh configuration file

.SH SEE ALSO
\fBqcp\fR(1), \fBssh_config\fR(5)

.UR https://docs.rs/qcp/latest/qcp/doc/performance/index.html
.UE

.SH AUTHOR
Ross Younger
";

    #[test]
    fn markdown() -> Result<()> {
        let output = PathBuf::from(
            std::env::var_os("QCP_MANPAGE_OUT_DIR")
                .or_else(|| std::env::var_os("OUT_DIR"))
                .ok_or(anyhow!("Neither QCP_MANPAGE_OUT_DIR nor OUT_DIR is set"))?,
        )
        .join("qcp.md");

        let md = clap_markdown::help_markdown::<CliArgs>();
        std::fs::write(&output, md)?;
        println!("Wrote to {output:?}");
        Ok(())
    }
}
