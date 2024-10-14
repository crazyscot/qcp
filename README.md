The Quic Copier (`qcp`) is an experimental
high-performance remote file copy utility for long-distance internet connections.

[//]: # (TODO: Badges, after first publication. Crates, docs, ci, license.)

## 📋 Features

- 🔧 Drop-in replacement for `scp` or `rcp`
- 🛡️ Similar security to `scp`, using existing, well-known mechanisms
- 🚀 Better throughput on congested networks

#### Platform support status

- Well tested: Debian and Ubuntu using OpenSSH
- Tested: Ubuntu on WSL
- Untested: OSX/BSD family
- Not currently supported: Windows

## 🧰 Getting Started

* You must have ssh access to the target machine.
* Install the `qcp` binary on both machines. It needs to be in your `PATH` on the remote machine.
* Run `qcp --help-buffers` and follow its instructions.

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

You can run the program like this:

```bash
$ qcp my-server:/tmp/testfile /tmp/
⠂ Transferring data, instant rate: 2.1MB/s
testfile ████████████████████████████████████░░░░░░░░░░░░░░░░░░░░░░░░ 1s @ 6.71 MB/s [60%/10.49 MB]
```

The program uses ssh to connect to the target machine and run `qcp --server`. ssh will check the remote host key and prompt you for a password or passphrase in the usual way.

The default options are for a 100Mbit connection, with 300ms round-trip time to the target server.

You may care to set the options for your internet connection. For example, if you have 300Mbit/s (37.5MB/s) download and 100Mbit/s (12.5MB/s) upload:

```bash
qcp my-server:/tmp/testfile /tmp/ --tx 12M --rx 37M
```

[//]: # (TODO: link to crates.rs: Performance is a tricky subject, more fully discussed in ...)

## ⚖️ License

The initial release is made under the [GNU Affero General Public License](LICENSE).

## 🧑‍🏭 Contributing

Feel free to report bugs via the [bug tracker].

I'd particularly welcome performance reports from BSD/OSX users as that's not a platform I use regularly.

While suggestions and feature requests are welcome, please be aware that I mostly work on this project in my own time.

## 💸 Supporting the project

If you find this software useful and would like to support me, please consider [buying me a coffee] (or via [ko-fi]).

If you're a business and need a formal invoice for your accountant, my freelancing company can issue the paperwork. For this, and any other commercial enquiries (alternative licensing, support, sponsoring features) please get in touch.

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
