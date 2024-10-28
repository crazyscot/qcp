[![Crates.io](https://img.shields.io/crates/v/qcp.svg)](https://crates.io/crates/qcp)
![GitHub code size in bytes](https://img.shields.io/github/languages/code-size/crazyscot/qcp)
[![Build status](https://github.com/crazyscot/qcp/actions/workflows/ci.yml/badge.svg)](https://github.com/crazyscot/qcp/actions/workflows/ci.yml)
[![Documentation](https://docs.rs/qcp/badge.svg)](https://docs.rs/qcp/)
[![License](https://img.shields.io/badge/License-AGPL_v3-orange.svg)](LICENSE)

The QUIC Copier (`qcp`) is an experimental
high-performance remote file copy utility for long-distance internet connections.

## 📋 Features

- 🔧 Drop-in replacement for `scp` or `rcp`
- 🛡️ Similar security to `scp`, using existing, well-known mechanisms
- 🚀 Better throughput on congested networks

#### Platform support status

- Well tested: Debian and Ubuntu on x86_64, using OpenSSH
- Tested: Ubuntu on WSL; aarch64 (Raspbian)
- Untested: OSX/BSD family
- Not currently supported: Windows

## 🧰 Getting Started

* You must have ssh access to the target machine.
* Install the `qcp` binary on both machines. It needs to be in your `PATH` on the remote machine.
* Run `qcp --help-buffers` and follow its instructions.

### Installing pre-built binaries

These can be found on the [latest release](https://github.com/crazyscot/qcp/releases/latest).

* Linux x86_64: x86_64-unknown-linux-musl
* Linux aarch64: aarch64-unknown-linux-musl

The binaries are statically linked. Linux builds should work on all recent distributions, as long as you have selected the correct CPU architecture.

### Installation from source

Install it from crates.io using `cargo`:

```bash
cargo install qcp
```

Or, clone the repo and build it manually:

```bash
git clone https://github.com/crazyscot/qcp
cd qcp
cargo build --release --locked
```

#### If you are new to Rust and don't have the tools installed

* Install the `rustup` tool via your package manager, or see [Rust installation](https://www.rust-lang.org/tools/install)
* `rustup toolchain install stable`
* Proceed as above

## ⚙️ Usage

The basic syntax is the same as scp or rcp.

```
qcp [OPTIONS] <SOURCE> <DESTINATION>
```

The program has a comprehensive help message, accessed via `qcp -h` (brief) or `qcp --help` (long form).

For example:

```bash
$ qcp my-server:/tmp/testfile /tmp/
⠂ Transferring data, instant rate: 2.1MB/s
testfile ████████████████████████████████████░░░░░░░░░░░░░░░░░░░░░░░░ 1s @ 6.71 MB/s [60%/10.49 MB]
```

**The program uses the ssh binary on your system to connect to the target machine**.
ssh will check the remote host key and prompt you for a password or passphrase in the usual way.

#### Tuning

By default qcp is tuned for a 100Mbit connection, with 300ms round-trip time to the target server.

Various network tuning options are available.

For example, if you have 300Mbit/s (37.5MB/s) download and 100Mbit/s (12.5MB/s) upload, you might use these options:

```bash
qcp my-server:/tmp/testfile /tmp/ --rx 37M --tx 12M
```

Performance tuning can be a tricky subject. See the [performance] documentation.

## 📖 How qcp works

The brief version:

1. We ssh to the remote machine and run `qcp --server` there
1. Both sides generate a TLS key and exchange self-signed certs over the ssh pipe between them
1. We use those certs to set up a QUIC session between the two
1. We transfer files over QUIC

The [protocol] documentation contains more detail and a discussion of its security properties.

## ⚖️ License

The initial release is made under the [GNU Affero General Public License](LICENSE).

## 🧑‍🏭 Contributing

Feel free to report bugs via the [bug tracker].

I'd particularly welcome performance reports from BSD/OSX users as that's not a platform I use regularly.

While suggestions and feature requests are welcome, please be aware that I mostly work on this project in my own time.

## 💸 Supporting the project

If you find this software useful and would like to say thank you, please consider [buying me a coffee] or [ko-fi]. [Github sponsorship] is also available.

If you're a business and need a formal invoice for your accountant, my freelancing company can issue the paperwork.
For this, and any other commercial enquiries (alternative licensing, support, etc) please get in touch, to `qcp@crazyscot.com`.

Please also consider supporting the galaxy of projects this work builds upon.
Most notably, [Quinn] is a pure-Rust implementation of the [QUIC] protocol, without which qcp simply wouldn't exist in its current form.

### 💡 Roadmap

Some ideas for the future, in no particular order:

* A local config mechanism, so you don't have to type out the network parameters every time
* Support for copying multiple files (e.g. shell globs or `scp -r`)
* Windows native support, at least for client mode
* Firewall/NAT traversal
* Interactive file transfer (akin to `ftp`)
* Smart file copy using the `rsync` protocol or similar (send only the sections you need to)
* Graphical interface for ftp mode
* Review the protocol and perhaps pivot to using capnp RPC
* Bind a daemon to a fixed port, for better firewall/NAT traversal properties but at the cost of having to implement user authentication.
* _The same thing we do every night, Pinky. We try to take over the world!_

[bug tracker]: https://github.com/crazyscot/qcp/issues
[quic]: https://quicwg.github.io/
[Quinn]: https://opencollective.com/quinn-rs
[rfc9000]: https://www.rfc-editor.org/rfc/rfc9000.html
[buying me a coffee]: https://buymeacoffee.com/rossyounger
[ko-fi]: https://ko-fi.com/rossyounger
[protocol]: https://docs.rs/qcp/latest/qcp/protocol/index.html
[performance]: https://docs.rs/qcp/latest/qcp/doc/performance/index.html
[Github sponsorship]: https://github.com/sponsors/crazyscot?frequency=recurring&sponsor=crazyscot
