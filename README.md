[![Crates.io](https://img.shields.io/crates/v/qcp.svg)](https://crates.io/crates/qcp)
[![Build status](https://github.com/crazyscot/qcp/actions/workflows/ci.yml/badge.svg)](https://github.com/crazyscot/qcp/actions/workflows/ci.yml)
[![Documentation](https://docs.rs/qcp/badge.svg)](https://docs.rs/qcp/)
[![codecov](https://codecov.io/gh/crazyscot/qcp/graph/badge.svg?token=RNYUNDHTQI)](https://codecov.io/gh/crazyscot/qcp)
[![License](https://img.shields.io/badge/License-AGPL_v3-orange.svg)](LICENSE)
![GitHub code size in bytes](https://img.shields.io/github/languages/code-size/crazyscot/qcp)

The QUIC Copier (`qcp`) is an experimental
high-performance remote file copy utility for long-distance internet connections.

## 📋 Features

- 🔧 Drop-in replacement for `scp`
- 🛡️ Similar security to `scp`, using existing, well-known mechanisms
- 🚀 Better throughput on congested networks

### News

- **(New in 0.4)**
  * Added builds for [OSX](https://docs.rs/qcp/latest/qcp/doc/osx/index.html), [Windows](https://docs.rs/qcp/latest/qcp/doc/windows/index.html) and BSD
  * New features: Explicit username with `-l <login-name>`; ssh subsystem mode
- **(New in 0.3)**
  * Negotiate transport setting by combining configuration from both sides
  * Protocol encoding changed to [BARE], removing the dependency on capnp.

For a full list of changes, see the [changelog].

#### Platform support status

At the time of writing, qcp has only been tested with the OpenSSH server and client.

| Platform         | CPU architecture | Status |
| --------         | ------ | ----- |
| Debian, Ubuntu   | x86_64, aarch64 | Well tested |
| Debian, Ubuntu   | Others | Untested, should work |
| NixOS            | x86_64 | Well tested |
| NixOS            | aarch64 | Untested, should work |
| Other Linux      | x86_64, aarch64 | Only indirectly tested** |
| OSX              | x86_64 | Tested |
| OSX              | aarch64 | Untested, should work |
| FreeBSD          | x86_64 | Buildable as of 31/3/25, but untested |
| NetBSD           | | Ought to be buildable* |
| OpenBSD          | | Ought to be buildable* |
| Windows 11       | x86_64 | Tested |

*_Note that the *BSD family are not tier 1 Rust platforms, so there is no guarantee that things will work properly. As of 31/3/25 some of these platforms have cross-compilation issues; I haven't tried native compilation._

** _The binaries in the tarballs are the exact same binaries found in the Debian/Ubuntu packages. They are static-linked, independent of kernel and libc._

## 🧰 Getting Started

* You must have ssh access to the target machine.
  - You must be able to exchange UDP packets with the target on a given port.
  - If the local machine is behind connection-tracking NAT, things usually work just fine. This is the case for the vast majority of home and business network connections.
  - You can tell qcp to use a particular port range if you need to.
* Install the `qcp` binary on both machines. It needs to be in your `PATH` on the remote machine.
* Run `qcp --help-buffers` and follow its instructions.
* Platform-specific notes exist for:
  * [OSX](https://docs.rs/qcp/latest/qcp/doc/performance/osx.html)

### Installing by package

These can be found on the [latest release](https://github.com/crazyscot/qcp/releases/latest) page.

| Platform |  CPU  | Packaging   |
| -------- | ----- | ------- |
| Debian & Ubuntu | x86_64, aarch64 | Deb packages |
| Other Linux | x86_64, aarch64 | Use the unknown-linux-musl tarballs (they are cross-platform, static linked) |
| Nix | * | Flakes are provided: `nix shell 'github:crazyscot/qcp?dir=nix'` |
| NixOS | * | In progress; refer to [pr#361923](https://github.com/NixOS/nixpkgs/pull/361923) |
| OSX  | x86_64, aarch64 | Tarball (code signing TBD) |
| Windows | x86_64 | Zipfile (code signing TBD) |

### Building from source

Prerequisites:
* Rust toolchain (see the next section if you don't know how to install this)
* A C compiler that the [cc crate](https://docs.rs/cc/latest/cc/#compile-time-requirements) can use

You can install the package from source in one step using `cargo`:

```bash
cargo install --locked qcp
```

Or, if you prefer, clone the source repository in the usual way, then `cargo build --release --locked`.

#### If you are new to Rust and don't have the tools installed

* Install the `rustup` tool via your package manager, or see [Rust installation](https://www.rust-lang.org/tools/install)
* `rustup toolchain install stable`
* `cargo install --locked qcp`

## ⚙️ Usage

The basic syntax is the same as scp or rcp.

```
qcp [OPTIONS] <SOURCE> <DESTINATION>
```

The program has a comprehensive help message, accessed via `qcp -h` (brief) or `qcp --help` (long form).

For example:

```text
$ qcp my-server:/tmp/testfile /tmp/
⠂ Transferring data                                                           2.1MB/s (last 1s)
testfile ████████████████████████████████████░░░░░░░░░░░░░░░░░░░░░░░░ 1s @ 6.71 MB/s [10.49 MB]
```

Things you should know:

* **qcp uses the ssh binary on your system to connect to the target machine**.
ssh will check the remote host key and prompt you for a password or passphrase in the usual way.

* **qcp will read your ssh config file** to resolve any Hostname aliases you may have defined there.
The idea is, if you can `ssh` to a host, you should also be able to `qcp` to it.
However, some particularly complicated ssh config files may be too much for qcp to understand.
(In particular, `Match` directives are not currently supported.)
In that case, you can use `--ssh-config` to provide an alternative configuration (or set it in your qcp configuration file).

#### Tuning

qcp's default configuration is for a **100Mbit symmetric connection**, with **300ms round-trip time** to the target server.

Naturally, you will get better performance if you set things up for your actual network connection.

For example, if you have 300Mbit/s (37.5MB/s) download and 100Mbit/s (12.5MB/s) upload, you might try this on the command line:

```bash
qcp my-server:/tmp/testfile /tmp/ --rx 37.5M --tx 12.5M
```


Performance tuning can be a tricky subject. See the [performance] documentation, and our recommended approach to
[building a configuration].

#### Persistent configuration

The useful options -- those you might want to use regularly including `rx`, `tx` and `rtt` -- can be specified
in a configuration file. See [config] for details.

For the example above, you might put this into `~/.qcp.conf` or `/etc/qcp.conf`:

```text
Host *
Rx 37.5M
Tx 12.5M
```


## 📖 How qcp works

The brief version:

1. We ssh to the remote machine and run `qcp --server` there (with no further args, i.e. you can use `command="qcp --server"` in your authorized_keys file)
   * there is also an option to use ssh subsystem mode, if you prefer to set that up
1. Both sides generate a TLS key and exchange self-signed certs over the ssh pipe between them
1. We use those certs to set up a QUIC session between the two
1. We transfer files over QUIC

The [protocol] documentation contains more detail and a discussion of its security properties.

## 📘 Project Policies

### ⚖️ License

This project is released publicly under the [GNU Affero General Public License](LICENSE).

Alternative license terms can be made available on request on a commercial basis (see below).

### 📋 SBOM

qcp draws on the Rust ecosystem. The various components used within the application are available under their own licenses.

To help you understand the legals and provide data for analysis tools, we automatically generate a Software Bill of Materials at build time.
This is included in the build bundles as `qcp.cdx.xml`. It's a CycloneDX BOM; for tools to process this information, see [CycloneDX Tool Center](https://cyclonedx.org/tool-center/).

### 🧑‍🏭 Bugs, Features & Contributions

Bug reports and feature requests are welcome, please use the [issue] tracker.

- It may be useful to check the [issues list] and the [discussions] first in case somebody else has already raised it.
- Please be aware that I mostly work on this project in my own time.

🚧 If you're thinking of contributing code, please read [CONTRIBUTING.md].

#### Help wanted: MacOS/BSD/Windows

I'd particularly welcome performance reports here, as they are not platforms I use regularly.

### 📑 Version number and compatibility

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) 2.0.0.

In its initial experimental phase, the major number will be kept at 0.
Breaking changes will be noted in the [changelog] and will trigger a minor version bump.

The project will move to version 1.x when the protocol has stabilised. After 1.0, breaking changes will trigger a major version bump.

### Unsafe Rust

Any uses of unsafe Rust will be kept to a bare minimum and carefully reviewed.

Test code (protected by `#[cfg(test)]`) may be exempted from this policy.
However we prefer that unsafe test code lives in the subcrate `qcp-unsafe-tests`.

Therefore, unsafe Rust is currently forbidden in the main qcp crate.

## 💸 Supporting the project

If you find this software useful and would like to say thank you, please consider [buying me a coffee] or [ko-fi]. [Github sponsorship] is also available.

Please also consider supporting the galaxy of projects this work builds upon.
Most notably, [Quinn] is a pure-Rust implementation of the [QUIC] protocol, without which qcp simply wouldn't exist in its current form.

If you're a business and need a formal invoice for your accountant, my freelancing company can issue the paperwork.
For this, and any other commercial enquiries please get in touch, to `qcp@crazyscot.com`. We would be pleased to discuss commercial terms for:

* Alternative licensing
* Support
* Sponsoring feature development

## 💡 Future Directions

Some ideas for the future, in no particular order:

* File checksum/hash checking/reporting
* Support for copying multiple files (e.g. shell globs or `scp -r`)
* Windows native support
* Firewall/NAT traversal
* Interactive file transfer (akin to `ftp`)
* Smart file copy using the `rsync` protocol or similar (send only the sections you need to)
* Graphical interface for ftp mode
* Bind a daemon to a fixed port, for better firewall/NAT traversal properties but at the cost of having to implement user authentication.
* _The same thing we do every night, Pinky. We try to take over the world!_

See also the [issues list].

[issue]: https://github.com/crazyscot/qcp/issues/new/choose
[issues list]: https://github.com/crazyscot/qcp/issues
[discussions]: https://github.com/crazyscot/qcp/discussions
[quic]: https://quicwg.github.io/
[Quinn]: https://opencollective.com/quinn-rs
[rfc9000]: https://www.rfc-editor.org/rfc/rfc9000.html
[buying me a coffee]: https://buymeacoffee.com/rossyounger
[ko-fi]: https://ko-fi.com/rossyounger
[config]: https://docs.rs/qcp/latest/qcp/config/index.html
[protocol]: https://docs.rs/qcp/latest/qcp/protocol/index.html
[performance]: https://docs.rs/qcp/latest/qcp/doc/performance/index.html
[building a configuration]: https://docs.rs/qcp/latest/qcp/doc/performance/index.html#building-a-configuration
[Github sponsorship]: https://github.com/sponsors/crazyscot?frequency=recurring&sponsor=crazyscot
[BARE]: https://www.ietf.org/archive/id/draft-devault-bare-11.html
[changelog]: https://github.com/crazyscot/qcp/blob/HEAD/CHANGELOG.md
[CONTRIBUTING.md]: https://github.com/crazyscot/qcp/blob/HEAD/CONTRIBUTING.md
