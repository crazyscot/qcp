# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

