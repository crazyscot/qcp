# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.3](https://github.com/crazyscot/qcp/compare/v0.3.2...v0.3.3)

### 🛡️ Security

- bump ring from 0.17.11 to 0.17.13 to address potential overflow panic - ([06cd47b](https://github.com/crazyscot/qcp/commit/06cd47b))


## [0.3.2](https://github.com/crazyscot/qcp/compare/v0.3.1...v0.3.2)

### 🏗️ Build, packaging & CI

- Fix readme in Cargo.toml - ([9e16c80](https://github.com/crazyscot/qcp/commit/9e16c80359b9508dd46651384971bb58b9acb210))


## [0.3.1](https://github.com/crazyscot/qcp/compare/v0.3.0...v0.3.1)

### ⛰️ Features

- Add Nix flake (#63) - ([ef37a12](https://github.com/crazyscot/qcp/commit/ef37a128c1cfe561be9617e5fcee1de51840faf8))

### 📚 Documentation

- Generate man page - ([544b07a](https://github.com/crazyscot/qcp/commit/544b07a79bd38e091c1c7bcb778cdc389a16b353))
- Updates for 0.3 series - ([1099e99](https://github.com/crazyscot/qcp/commit/1099e99a54087937841df2ec172bac7550504a68))

### 🏗️ Build, packaging & CI

- Generate CycloneDX SBOM files and include in release bundles - ([f2d4626](https://github.com/crazyscot/qcp/commit/f2d462607f035f317cb700ac3da9b7e9c9ef64e0))
- Generate licenses.html, include in bundles - ([e3ce44f](https://github.com/crazyscot/qcp/commit/e3ce44f063fed012e1c6d87ccbaac15156badb40))
- Set up xtasks for man page, licenses.html, dummy debian changelog
- Rearrange source into a workspace - ([e4b05bf](https://github.com/crazyscot/qcp/commit/e4b05bf40c033ede1560d1dbf8c85ce9c20d6b6e))

### ⚙️ Miscellaneous Tasks

- Fix clippy warnings for rust 1.85 - ([bfae2d5](https://github.com/crazyscot/qcp/commit/bfae2d5a65e6ba2b38c828d37ca2d53b8c920d07))
- Lintian fix for qcp_config.5 - ([355c539](https://github.com/crazyscot/qcp/commit/355c539ae3a3c924b7be91abd54c9e1799ebeaa3))

### 🚜 Refactor

- Rework --help-buffers mode - ([d461eaf](https://github.com/crazyscot/qcp/commit/d461eaf50ef3d288d718817c9087b014b797caed))

## [0.3.0](https://github.com/crazyscot/qcp/compare/v0.2.1...v0.3.0)

### ⛰️ Features

- [**breaking**] Compute a negotiated transport configuration based on optional preferences from either side (#32) - ([0af6eff](https://github.com/crazyscot/qcp/commit/0af6eff424adfc82d43ae37d0ea5f8ad3a84d284))
- Server can send a failure message during control protocol - ([445ec4e](https://github.com/crazyscot/qcp/commit/445ec4ef71062118ec33e785070b8d14854512b3))
- Add --remote-config mode - ([6de53e1](https://github.com/crazyscot/qcp/commit/6de53e157a2e37cef88c9b19c722f2d4fd363fa1))
- Server uses the ssh remote client address to select configuration - ([b06475b](https://github.com/crazyscot/qcp/commit/b06475bf9f30da50251eea063076e3ff100a931e))
- Add --dry-run mode - ([b74f080](https://github.com/crazyscot/qcp/commit/b74f08010bf254661ff77f742fcc80489a9ef271))

### 🐛 Bug Fixes

- Client correctly marshalls remote_port as None when not specified - ([d1b0054](https://github.com/crazyscot/qcp/commit/d1b0054492ffcf2260c6857b9270d19c5c38fb32))
- Resolve --tx 0 correctly - ([573c9b4](https://github.com/crazyscot/qcp/commit/573c9b4fd115624df98ee905d92a779ec5acb5a6))
- Always ssh to the entered ssh hostname, so we respect any aliasing in ssh_config - ([f9421a5](https://github.com/crazyscot/qcp/commit/f9421a55e95fadf47aa2a2643749fc85a41d3f7a))
- Username is not part of the hostname when parsing config - ([3544219](https://github.com/crazyscot/qcp/commit/3544219fdae3f3f949a7ba06f8defad9ab993e73))

### 🎨 Styling

- Improve server error messages (show detail as well as context) - ([d010535](https://github.com/crazyscot/qcp/commit/d0105354a150f8ab292dd381fb293e78f4a766ff))
- Improve tracing and debug output - ([663bc3e](https://github.com/crazyscot/qcp/commit/663bc3e6fc880c36d9c95d7cf9e02e0dae997293))

### 🧪 Testing

- Complete test coverage in protocol module - ([b3473b0](https://github.com/crazyscot/qcp/commit/b3473b099f4d954635d73b56a0a292cad38d3b11))
- Add coveralls - ([abac087](https://github.com/crazyscot/qcp/commit/abac087bfe7b9ec2c6a1ded4d41095463b9313b3))
- Add unit tests for client::options - ([bffba19](https://github.com/crazyscot/qcp/commit/bffba19eaf731e7cbff1faf3a7ac5f2c030b2dea))
- Fill in job.rs unit tests - ([ae835bc](https://github.com/crazyscot/qcp/commit/ae835bceffbff1af930f826b0bbe61bbceff236f))
- Add local coverage script - ([57f044a](https://github.com/crazyscot/qcp/commit/57f044a90b842b759014892e5b43935f89bd8027))

### ⚙️ Miscellaneous Tasks

- [**breaking**] Change protocol encoding from capnp to BARE - ([85a1243](https://github.com/crazyscot/qcp/commit/85a124339763431697bd56d3987e58f39da787d9))
- Size limits for on-wire messages - ([9f6ef11](https://github.com/crazyscot/qcp/commit/9f6ef1163b6b109344dfd1c7e142ad77cc697035))
- Improve error message when remote uses the old protocol - ([7cc27de](https://github.com/crazyscot/qcp/commit/7cc27de5496dd3378ca4be86636880a5179d5077))

### 🚜 Refactor

- Config combination produces a first-class Figment - ([8bdb623](https://github.com/crazyscot/qcp/commit/8bdb623d7e06faaebf3520b50e8e16a37d9568c3))
- Deduplicate configuration validation logic - ([af320be](https://github.com/crazyscot/qcp/commit/af320be2e884383d33fbfbaf51207e0c8eb76226))
- Tidyup config manager and client child process handling - ([e44a54f](https://github.com/crazyscot/qcp/commit/e44a54ffc97a6c660635deeda267e09327e4efc6))
- Make PortRange.combine() more coherent - ([c2877e5](https://github.com/crazyscot/qcp/commit/c2877e5ffb417c29f84db9b3fb2526e824f34e36))
- Various tidyups in support of transport negotiation - ([cd4a30a](https://github.com/crazyscot/qcp/commit/cd4a30a4a9f077307f1c8485c187aaa55eb19654))
- Drop expanduser; do the work in-house instead - ([43cfbd3](https://github.com/crazyscot/qcp/commit/43cfbd34a88acb360592dd7326ac259e43344090))
- Remove MODE_OPTIONS which wasn't used in a well-defined way - ([390ee6f](https://github.com/crazyscot/qcp/commit/390ee6fc4d8afc714e43c851d5a8b058828dd90c))

## [0.2.1](https://github.com/crazyscot/qcp/compare/v0.2.0...v0.2.1)

### ⛰️ Features

- Improved parsing flexibility for bandwidth (12.3M, etc) - ([389b21a](https://github.com/crazyscot/qcp/commit/389b21a1c2b5f0b744ac4e611146d7b416061103))

### 🐛 Bug Fixes

- Validate configuration before attempting to use - ([d3f13ec](https://github.com/crazyscot/qcp/commit/d3f13ecf2e4ec2b82d3c2a344b965dff51933e80))

### 🎨 Styling

- Align console messages outside of tracing - ([e9e651a](https://github.com/crazyscot/qcp/commit/e9e651a9280922c49723491643e832a2ffdcbab9))

### 🚜 Refactor

- Align return codes from cli_main, server_main and client_main - ([7f2b243](https://github.com/crazyscot/qcp/commit/7f2b24316f04d87b975c18fd8db61da93cdf57aa))
- SshConfigError uses thiserror to implement standard Error - ([16ef7ed](https://github.com/crazyscot/qcp/commit/16ef7ed8c7133c8625df85f171e3a0befcb382f7))


## [0.2.0](https://github.com/crazyscot/qcp/compare/v0.1.3...v0.2.0)

### ⛰️ Features

- [**breaking**] Configuration file system ([#17](https://github.com/crazyscot/qcp/pull/17)) - ([0baf2ba](https://github.com/crazyscot/qcp/commit/0baf2bab9236c9f49050cc3eda191c9fcd1e9a72))
- Look up host name aliases in ssh_config ([#22](https://github.com/crazyscot/qcp/pull/22)) - ([46c450d](https://github.com/crazyscot/qcp/commit/46c450d63a468222108c7ea79fb0b1aca90f156a))
- Allow user to specify the time stamp format for printed/logged messages - ([4eaf2ec](https://github.com/crazyscot/qcp/commit/4eaf2ecd101c9302b1ca9c4e25d2f6d4b4bdd481))

### 🐛 Bug Fixes

- Use correct format for the remote endpoint network config debug message - ([183e5fb](https://github.com/crazyscot/qcp/commit/183e5fba9cdd86a4a71892a4d66244da736f6ba6))
- Always use the same address family with ssh and quic - ([084904d](https://github.com/crazyscot/qcp/commit/084904dba387f75b35a108edf6cbc1b883c80743))

### 📚 Documentation

- Tidy up --help ordering, update man pages, tidy up doc comments - ([3837827](https://github.com/crazyscot/qcp/commit/383782768c00ae19d3e2dd0b9d0c93e60d6ec680))
- Update project policies and notes - ([399422b](https://github.com/crazyscot/qcp/commit/399422bd8da13485d316d45178d6a81697622f3d))

### 🎨 Styling

- Show Opening control channel message - ([4d14a26](https://github.com/crazyscot/qcp/commit/4d14a26589779f4bce1fabee8d1a0d9e5e7d3b3d))

### 🏗️ Build, packaging & CI

- Build rust binaries with --locked - ([5f0af1f](https://github.com/crazyscot/qcp/commit/5f0af1fba0e5cbb27a143908610c6e3361670c2b))
- Set git_release_draft=true, update MAINTENANCE.md - ([a25bf8b](https://github.com/crazyscot/qcp/commit/a25bf8ba54cef0546f3289e1374151defe0a51b0))
- Add cargo doc task to include private items; fix that build - ([c8298e2](https://github.com/crazyscot/qcp/commit/c8298e2ab7e4ee032c76b2888e79fb25d6390a93))
- Speed up link times - ([c6465ad](https://github.com/crazyscot/qcp/commit/c6465ad290ee7ec9fb7ee5fdc250e34730ffd106))
- Add Debian postinst script ([#13](https://github.com/crazyscot/qcp/pull/13)) - ([1a4e10e](https://github.com/crazyscot/qcp/commit/1a4e10ec92b4707b55ab4aaed1eda636e374c120))

### ⚙️ Miscellaneous Tasks

- Add feature flag to enable rustls logging (on by default) - ([4ac1774](https://github.com/crazyscot/qcp/commit/4ac177479e38a164de9e97390f3c2de3987c0050))
- Make HumanU64 parse errors more useful - ([63bf2f2](https://github.com/crazyscot/qcp/commit/63bf2f2ef2902ef1db211323e041b471e32454bc))
- Make PortRange parse errors more useful - ([013ea2b](https://github.com/crazyscot/qcp/commit/013ea2bedc016341c869227bfb12461514d99cc7))
- Update dependencies

## [0.1.3](https://github.com/crazyscot/qcp/compare/v0.1.2...v0.1.3)

### 🐛 Bug Fixes

- *(deps)* Bump rustls from 0.23.16 to 0.23.18 ([#15](https://github.com/crazyscot/qcp/pull/15)) - ([e333abc](https://github.com/crazyscot/qcp/commit/e333abc230528f2172cc2bf9605c5a5b2357d9fc))

### 📚 Documentation

- Add note about build prerequisite - ([6b176c9](https://github.com/crazyscot/qcp/commit/6b176c990d7b29d2dc5af623e3f430c2ee1bdc85))

### ⚙️ Miscellaneous Tasks

- *(build)* Fix autopublish of Debian packages - ([74b3ea6](https://github.com/crazyscot/qcp/commit/74b3ea6a7be2da3093d4a75a1e92b29946d203ad))


## [0.1.2](https://github.com/crazyscot/qcp/compare/v0.1.1...v0.1.2)

### 📚 Documentation

- Add build group to cliff config - ([603b6b6](https://github.com/crazyscot/qcp/commit/603b6b6726a86e69b584d936dc96175e471c3734))

### 🏗️ Build & CI

- Fix release workflow syntax - ([294bac3](https://github.com/crazyscot/qcp/commit/294bac32071f936e677bc4224b71ae5257975c99))
- Make build script less panicky - ([0d3ab56](https://github.com/crazyscot/qcp/commit/0d3ab56ffdf21acc49df39b715cada9fde9b14b3))


## [0.1.1](https://github.com/crazyscot/qcp/compare/v0.1.0...v0.1.1)

### ⛰️ Features

- Suppress RTT warning unless it's at least 10% worse than configuration - ([47be5a5](https://github.com/crazyscot/qcp/commit/47be5a5fe9b1b1938d147ead06332b870a39cce4))

### 🐛 Bug Fixes

- Autogenerate version string correctly in CI - ([64dfcea](https://github.com/crazyscot/qcp/commit/64dfcead3f3e24652e278f0c4d4c260b96b6e549))

### 🚜 Refactor

- Combine the capnp invocations - ([2bea195](https://github.com/crazyscot/qcp/commit/2bea19568e332138c08f105192166ffcb16f37c9))

### 📚 Documentation

- Add initial man page - ([61cf453](https://github.com/crazyscot/qcp/commit/61cf4535103ba5fadebc63af7e1e826ebf1532ec))

### ⚡ Performance

- Use jemallocator on musl 64-bit builds - ([83e1e58](https://github.com/crazyscot/qcp/commit/83e1e58c1159b8bf1659673b9cc736713740ee70))

### 🎨 Styling

- Move instant speed readout to the right, remove %age - ([dc68383](https://github.com/crazyscot/qcp/commit/dc683838bd22655c73d5d16e86a527252dd0550c))

### ⚙️ Miscellaneous Tasks

- *(ci)* Align ci and release workflows - ([d16d38a](https://github.com/crazyscot/qcp/commit/d16d38a7aeb7629a76151ae4eb69d1d1b28cd671))
- Remove spurious cache key - ([7e64feb](https://github.com/crazyscot/qcp/commit/7e64feb030ef65d979e59b891ac68ee43414e89d))
- Build debian package - ([435b6b5](https://github.com/crazyscot/qcp/commit/435b6b587adb0581cec36caccbcf5a8048b0403c))
- Add aarch64 build ([#7](https://github.com/crazyscot/qcp/pull/7)) - ([863eb71](https://github.com/crazyscot/qcp/commit/863eb71a24a7f08f35c342e554b3b87fb0bbf751))
- Tidy up CI, add release workflow ([#6](https://github.com/crazyscot/qcp/pull/6)) - ([dedfe22](https://github.com/crazyscot/qcp/commit/dedfe225f1c3c626380f6c28001ac641d1ca0ffe))


## [0.1.0]

### ⛰️ Features

- Support non-standard ssh clients and passthrough options - ([7e351f2](https://github.com/crazyscot/qcp/commit/7e351f24b710c263aa14c002647cd3fefa65e17e))
- Support user@host syntax - ([fd7aab7](https://github.com/crazyscot/qcp/commit/fd7aab71ec29781d8d4251c635ccd0a2c6571eaa))
- Option to select congestion control algorithm - ([da105d6](https://github.com/crazyscot/qcp/commit/da105d6429360e796a7fc74399cf90afe21b14da))
- IPv6 bare addressing i.e. [1:2:3::4]:file - ([bce0c44](https://github.com/crazyscot/qcp/commit/bce0c44112f4d3bba85c6cfdedf5859c37c34a2b))

### 📚 Documentation

- Initial set of rustdocs - ([129bd30](https://github.com/crazyscot/qcp/commit/129bd3073aa319e0fc9f9124dbc1d4798e4e05fe))

### 🎨 Styling

- Output statistics in human-friendly format - ([321a92d](https://github.com/crazyscot/qcp/commit/321a92d4e3aefed53c8af2867b0ee26a74c81801))
- Dynamically update spinner tick rate as a function of throughput - ([b62e0e7](https://github.com/crazyscot/qcp/commit/b62e0e7ec12f20eaa1af200cae2f9f687a7c91df))

