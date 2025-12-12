# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0](https://github.com/crazyscot/qcp/compare/v0.6.0...v0.7.0)

### ⛰️ Features

- Add remote qcp binary override flag - ([2440370](https://github.com/crazyscot/qcp/commit/24403702eabae55e319cb0e02c421f95cc44a63a))

### Deps

- Prefer the finished 'paste' crate to the unvetted 'pastey' - ([58367c8](https://github.com/crazyscot/qcp/commit/58367c82460fb984627f6e8030f2539ad9f80750))


## [0.6.0](https://github.com/crazyscot/qcp/compare/v0.5.2...v0.6.0)

### ⛰️ Features

- New output time format utc-micro / "UtcMicro" - ([1747596](https://github.com/crazyscot/qcp/commit/1747596025190bbf9688a62b3258da16ea01aadc))
- TLS RawPublicKey authentication - ([2b5b9c6](https://github.com/crazyscot/qcp/commit/2b5b9c6d0e75a222f8cf1baf2e32027270a2de93))

### 🐛 Bug Fixes

- Improve error reporting from the server main loop - ([ea1bd4a](https://github.com/crazyscot/qcp/commit/ea1bd4af6a8b318dd3bb1919c09b1a6fd6a7c493))
- Bandwidth configuration tx=0 special case - ([5806c35](https://github.com/crazyscot/qcp/commit/5806c354fe85ef6fc59bb638ad37efd0315f4283))
- Do not send ANSI reset codes when configured not to - ([7dc8efb](https://github.com/crazyscot/qcp/commit/7dc8efba5c86fa86ea8a1c0a5aecbebd268ea2f7))
- Make error output from invalid configuration files less confusing - ([6bb8f2d](https://github.com/crazyscot/qcp/commit/6bb8f2df17ebb2e341052dafc98b06b026ac0e34))
- Do not use ANSI colours in server mode - ([6dc1d0c](https://github.com/crazyscot/qcp/commit/6dc1d0c5287df5878bccaa09e145bb2793cab697))
- Report failures in --server mode less confusingly - ([cd90b46](https://github.com/crazyscot/qcp/commit/cd90b46de3a5125170a9620f625eca9da1b69e89))

### ⚡ Performance

- Automatic TLS cipher suite selection - ([c3461d0](https://github.com/crazyscot/qcp/commit/c3461d07731fd64a8a79956505d26f6dfe9ef4ec))

### 🧪 Testing

- Add regression test for repeatable CLI arguments ([#170](https://github.com/crazyscot/qcp/pull/170)) - ([43529c5](https://github.com/crazyscot/qcp/commit/43529c583377356ba6c46595d55641280f082217))
- Add wire marshalling checks for ServerMessageV2 and ClientMessageV2 - ([99fc072](https://github.com/crazyscot/qcp/commit/99fc0720200bf673cdf7fa811cb7b575355836ff))

### ⚙️ Miscellaneous Tasks

- If we receive bogus protocol data that is valid UTF-8, send it to debug output - ([694e396](https://github.com/crazyscot/qcp/commit/694e3966308942ab7f31a1bf0b2ce7798307f4fa))
- Remove unused trait impls - ([f087abe](https://github.com/crazyscot/qcp/commit/f087abefe5a72e1265213ab42d09917d6e53ed44))
- Tidy up boolean argument declarations - ([17ca32d](https://github.com/crazyscot/qcp/commit/17ca32d8e9b120e2affc58d157da719aa9a69eb0))
- Update MSRV to 1.88 - ([9cbf893](https://github.com/crazyscot/qcp/commit/9cbf893e2afce33c40c1bcd0b4fd0b5f042e386c))
- Remove SerializeAsString, no longer needed - ([f217248](https://github.com/crazyscot/qcp/commit/f21724897d375a3111589eaf68426179f1ae9db4))
- Give Optionalify the ability to replace serde attributes - ([1e5735e](https://github.com/crazyscot/qcp/commit/1e5735eb06f8ac53bff650536542faf89c1788e6))
- Add TaggedData::with_bytes(), with_str() - ([8fc003b](https://github.com/crazyscot/qcp/commit/8fc003bb7e8213b68e61264dadf90718d3e15883))
- Improve main loop error reporting - ([31b15a4](https://github.com/crazyscot/qcp/commit/31b15a48d37cc67b3c4c63f8612f688a0398ed72))

### 🚜 Refactor

- Remove EngineeringQuantity wrappers from config structure in favour of serialization helper - ([a0aea97](https://github.com/crazyscot/qcp/commit/a0aea972b1b3a137a81dc45e5c129d8c68245b86))
- Remove VecOrString wrapper type in favour of serialization helpers - ([5068263](https://github.com/crazyscot/qcp/commit/5068263a7c4f84bcdf020ec2ae86663e03d04594))
- Serialize PortRange directly, without a conversion type - ([4dae6e4](https://github.com/crazyscot/qcp/commit/4dae6e45c692ddf2d8b677dfd0d6be74de156dd8))
- Parsing for CongestionController - ([582986e](https://github.com/crazyscot/qcp/commit/582986e41729f15d95ffba8fb5e3c92025e0fede))
- Parsing for CredentialsType - ([86ba732](https://github.com/crazyscot/qcp/commit/86ba73255d6b2c85a34e5ce4508f8f0363b0eba9))
- Rationalise enum parsing - ([ff59ae0](https://github.com/crazyscot/qcp/commit/ff59ae0d14e717324cedb8d681ab37db276e59ad))

## [0.5.2](https://github.com/crazyscot/qcp/compare/v0.5.1...v0.5.2)

### 🛡️ Security

- Bump tracing-subscriber from 0.3.19 to 0.3.20 - ([7af8577](https://github.com/crazyscot/qcp/commit/7af85772b68265d96b9861ac7e20d9dd1a91fe3d))

## [0.5.1](https://github.com/crazyscot/qcp/compare/v0.5.0...v0.5.1)

### 🏗️ Build, packaging & CI

- Fix debian postinst script edge case - ([e37386c](https://github.com/crazyscot/qcp/commit/e37386ce0b0111de8337c20b08f6f3e58a4ea7da))

## [0.5.0](https://github.com/crazyscot/qcp/compare/v0.4.2...v0.5.0)

### ⛰️ Features

- [**breaking**] Make kernel UDP buffer size configurable - ([963541b](https://github.com/crazyscot/qcp/commit/963541bcd14d273d8f6604e2a8bf48be591f0b00))
  - **Linux users:** This change may cause warnings about buffer sizes requiring you to reconfigure your sysctls. Run `qcp --help-buffers` and follow its advice.
- [**breaking**] Remove some infrequently-used CLI short options - ([ecad214](https://github.com/crazyscot/qcp/commit/ecad21429c9dad6ca963bebcf0bafa19c1af0dfe))
- Add remote PMTU and RTT to --stats output (via ClosedownReport) - ([92d8675](https://github.com/crazyscot/qcp/commit/92d867599f11324f86bfd9bf674f1c298ddbeebd))
- --preserve option - ([24b8ef2](https://github.com/crazyscot/qcp/commit/24b8ef2943d1e54c310809c6a8b2506b06bba738))
- Add hidden --remote-trace option - ([9b022a3](https://github.com/crazyscot/qcp/commit/9b022a3448e7b530c584943d6474510a1c232dc0))
- Add NewReno congestion controller (protocol compat level 2) - ([e865a52](https://github.com/crazyscot/qcp/commit/e865a520140782db0e40b0de81d3d7e7a7ba61c4))
- Initial, min and max MTU can be configured - ([4919ca4](https://github.com/crazyscot/qcp/commit/4919ca4428f0b8f02e9be311e42080174887862d))
- Add packet loss detection thresholds as configurable settings - ([37e0f32](https://github.com/crazyscot/qcp/commit/37e0f329fd8415d4191855400c519e63195571e2))
- _(meta)_ --list-features option - ([c768fc9](https://github.com/crazyscot/qcp/commit/c768fc91a12b1bd8db9b4b011ca4f89ba6a4f00e))
- _(debug)_ Add the server's idea of MTU and RTT to --remote-debug - ([7161f6d](https://github.com/crazyscot/qcp/commit/7161f6d090cb666d8e43f354b1adb92141a81f44))
- _(protocol)_ Introduce Variant type - ([908346a](https://github.com/crazyscot/qcp/commit/908346ac36741fe437377049f4b35c45349a6f35))
- _(protocol)_ Add Compatibility level 2 - ([2d0c1ef](https://github.com/crazyscot/qcp/commit/2d0c1ef2381cdf3f8f373f1a81a273843be08064))

### 🐛 Bug Fixes

- Disallow reading from or writing to non-regular files (device nodes & sockets) - ([b2b036e](https://github.com/crazyscot/qcp/commit/b2b036e18c86132fb4c938633757cea81cac546f))
- CLI --ssh-config & -S options - ([145bb5d](https://github.com/crazyscot/qcp/commit/145bb5d5ff57d5a975aa9b3aca966971dd171d1e))
- Use correct system ssh config directory on Windows - ([6ec7001](https://github.com/crazyscot/qcp/commit/6ec70013ab76f11d04becbfbac90caad9a86a0ae))
- Do not allow RTT to be set to 0 - ([e27baa3](https://github.com/crazyscot/qcp/commit/e27baa31131a2a36443de121350dafe125f40b28))
- _(cli)_ Remove Markdown mark-up from --help output - ([6362bea](https://github.com/crazyscot/qcp/commit/6362bea74c25cd666e9e3ff8490ab408e833aafb))
- _(protocol)_ Don't crash on receipt of an unrecognised Status enum - ([baecbaa](https://github.com/crazyscot/qcp/commit/baecbaaf6618e29dfc4d56f1b2411f8c7cf225eb))
- _(protocol)_ Review and correct enum encoding - ([4a05d4a](https://github.com/crazyscot/qcp/commit/4a05d4a5f6c8d4d6cfd5995abff14c42b3ab1573))
- _(test)_ DNS lookup sometimes failed on Windows CI - ([79ec010](https://github.com/crazyscot/qcp/commit/79ec0102e3cc84aa304ef2af6117df23ec223008))
- _(ui)_ Don't report a peak rate of 0B/s on really fast transfers - ([d90364b](https://github.com/crazyscot/qcp/commit/d90364bb6b887c10a45552f9063033b287f13d90))

### 📚 Documentation

- Autogenerate CLI HTML doc - ([1163169](https://github.com/crazyscot/qcp/commit/11631697b41d772e1461393fbecdffb3d20164d9))
- Tidy/polish Rust docs - ([c639856](https://github.com/crazyscot/qcp/commit/c639856c7982fdd5ee5c3b01375d87d58c01c077))

### ⚡ Performance

- Add direction-of-travel indicator to ClientMessage - ([9c4b2ed](https://github.com/crazyscot/qcp/commit/9c4b2ed686115a7ee3248922aa335530d2f5a319))
- Set initial_rtt as 2x expected RTT - ([4866a98](https://github.com/crazyscot/qcp/commit/4866a98e65edb21fd71646f73835607d6d1f6ff5))
- Use a larger send window, in line with Quinn's default tuning - ([03f6161](https://github.com/crazyscot/qcp/commit/03f6161a9a690b96e900a695613868d2e935e9bf))

### 🎨 Styling

- Improve consistency of control channel debug output - ([cfcd6a6](https://github.com/crazyscot/qcp/commit/cfcd6a6287b982b429f770dced6a4503083f5401))
- Implement Display for TaggedData - ([1e10293](https://github.com/crazyscot/qcp/commit/1e1029352c8f695e1ae4f2a98aa58d694272a6aa))
- Reduce excess verbiage when reporting FileNotFound or IncorrectPermissions errors - ([35d53ce](https://github.com/crazyscot/qcp/commit/35d53ce539a73c2652d999e8c3e8bb5fdbf3314e))
- Make server-mode output more easily distinguishable - ([90d9659](https://github.com/crazyscot/qcp/commit/90d9659573130f0c926ec53e7a52ce749022a402))

### 🧪 Testing

- Add an integration test that simulates MITM attacks - ([f5e28f6](https://github.com/crazyscot/qcp/commit/f5e28f6fc21bd47bd024bef2c37946fc861c6e79))
- Move some CLI tests into integration test area - ([8fe67f9](https://github.com/crazyscot/qcp/commit/8fe67f99eb85ac1936cb01e4772943610998fcb7))
- Add wire marshalling regression tests for current protocol messages - ([51413ec](https://github.com/crazyscot/qcp/commit/51413eca7d0c8ad17b18ee85fda53923584c4149))
- Fill-in coverage in transport.rs - ([cef4f47](https://github.com/crazyscot/qcp/commit/cef4f47be71b0e0d87be977a55688d55131368fb))
- Fix compiler warnings on apple & windows - ([f3e90a2](https://github.com/crazyscot/qcp/commit/f3e90a232c4c84ad94455db0e30b24e83983c069))
- Exercise the main CLI modes and parser - ([eda8d12](https://github.com/crazyscot/qcp/commit/eda8d1270833397c5e38810b606f809eab852b3d))
- Add unit tests for the informational main modes - ([07fd94f](https://github.com/crazyscot/qcp/commit/07fd94fcacbcdd67db69e46ed2818f558540bae3))
- Refactor server_main, add a unit test - ([ef7f8c3](https://github.com/crazyscot/qcp/commit/ef7f8c375cad93f7e4ab346f147530410f211a7c))
- Refactoring server connection.handle_incoming, add a unit test - ([39be470](https://github.com/crazyscot/qcp/commit/39be470f2c0065b2995f4f5d521c720cb3d34c64))
- Add unit tests for server::connection_info, server::stream - ([72a0812](https://github.com/crazyscot/qcp/commit/72a081264e2f1a3b606d84f60173a567d87be4b5))
- Use pretty_assertions - ([12e41b6](https://github.com/crazyscot/qcp/commit/12e41b6d90eb104ffd1461bd34f9f00ee27c3f44))
- _(windows)_ Improve Windows config file path checks - ([bd42133](https://github.com/crazyscot/qcp/commit/bd42133ff86c775354daa1af727831a1d78a7840))

### 🏗️ Build, packaging & CI

- Rename output artifact names to be more user friendly - ([ec40671](https://github.com/crazyscot/qcp/commit/ec406710e9e3ce8a9442776e5d2e4a8bbc6e3ad9))
- Scaffolding & feature to expose internal test helpers for use by qcp-unsafe-tests - ([e947393](https://github.com/crazyscot/qcp/commit/e94739351b8ded5fd20c9dca6653e122229755b7))
- Disable tests where they don't work on mingw cross-compiles - ([064a4c9](https://github.com/crazyscot/qcp/commit/064a4c998c985640006691dd4f0fe447e25be759))

### ⚙️ Miscellaneous Tasks

- _(ci,docs)_ Fix cargo doc --document-private items, add it to CI checks - ([8113e84](https://github.com/crazyscot/qcp/commit/8113e8496d2b6c5ddffa46af009e1bf18295b78e))
- Use strum_macros:: consistently - ([8f1a39b](https://github.com/crazyscot/qcp/commit/8f1a39b2eca8f770b84238b5771ed07c941b4e82))
- Use if cfg!() instead of cfg_if! where possible - ([a787101](https://github.com/crazyscot/qcp/commit/a787101bf17127384c922e7c53b832c4294cbc16))
- Update template qcp.conf - ([594d8d7](https://github.com/crazyscot/qcp/commit/594d8d75626797f79b3923470f1c99429413a733))
- Tidy `os` module exports, OS notes - ([36dbbac](https://github.com/crazyscot/qcp/commit/36dbbac11d61b3962ad79c4f81947176ed2bf6d0))

### 🚜 Refactor

- _(test)_ Make the test suite cross-platform - ([47ba293](https://github.com/crazyscot/qcp/commit/47ba293ba65a399d3ba81effdab36d697cf64c5d))
- Add ergonomic constructors DataTag::with_unsigned,with_signed - ([af67621](https://github.com/crazyscot/qcp/commit/af67621b5c938b9cbcfafea6f95dce030ff2515e))
- Improve ThroughputMode consistency - ([88e4f3e](https://github.com/crazyscot/qcp/commit/88e4f3e4ee9c21ef586ccb3a8c90d79237bb8390))
- Add FindTag helper trait for Vec<TaggedData<>> - ([fb23c39](https://github.com/crazyscot/qcp/commit/fb23c39e96a86f937f317a74a342d6c475ba6356))
- Use derive_more::{Debug,Display} in control protocol - ([a179046](https://github.com/crazyscot/qcp/commit/a1790465954e60ba515ffbf6fde3734d666571b6))
- Move DataTag up a level so it can be common to both protocols - ([8d51685](https://github.com/crazyscot/qcp/commit/8d51685767cdffe169deb8a2dbc2b739959e790e))
- Split up client main_loop a bit more for readability & testability - ([a0e4b6c](https://github.com/crazyscot/qcp/commit/a0e4b6c59c33d24094ba0feb961cf76952503bff))
- Check_response() makes more sense as a function of Response - ([e24146f](https://github.com/crazyscot/qcp/commit/e24146ff32baaaaea2ece1eb1b4a8ab9b7496390))
- Client_main for testability and simplicity ([37620bb](https://github.com/crazyscot/qcp/commit/37620bbde0dcacb7d5a7fdec7c9c8a8a07265fb2)) ([f175d24](https://github.com/crazyscot/qcp/commit/f175d24666025bcb8e721ca53c1688173a58e654))
- Separate out process::Ssh into a generic ProcessWrapper and the ssh-specific parts - ([ecd1196](https://github.com/crazyscot/qcp/commit/ecd1196d855eb64000704e9aa5a56aea4a51b880))
- Tracing::setup() arguments - ([79faa48](https://github.com/crazyscot/qcp/commit/79faa48c30fa4c4b9aa00d26fa7f74931ca5d88c))
- Split apart server.rs for testability - ([31927fb](https://github.com/crazyscot/qcp/commit/31927fbc376e68ef9cc795cf4264cd597e62f616))
- Setup_tracing becomes a struct with a trait - ([e2a782f](https://github.com/crazyscot/qcp/commit/e2a782fd624dbd7bcfdc36a742fa5fd1426df08d))
- Use clap to create MainMode enum - ([49781fa](https://github.com/crazyscot/qcp/commit/49781fa1d2e2c918b643e7368bebe6f639fb6fe8))

## [0.4.2](https://github.com/crazyscot/qcp/compare/v0.4.1...v0.4.2)

### ⛰️ Features

- Report peak transfer speed - ([2d4c25f](https://github.com/crazyscot/qcp/commit/2d4c25fca6c3bb0b0410318bd0af684807d30ccc))

### 🐛 Bug Fixes

- Pre-flight configuration validation when not all fields are set - ([e3eb0bd](https://github.com/crazyscot/qcp/commit/e3eb0bd9648181e621759fc9d7eac6d1e4255c55))

### ⚙️ Miscellaneous Tasks

- Update readme - ([f797e7d](https://github.com/crazyscot/qcp/commit/f797e7d8a32be7a53688de1d4f961c25261d484f))

## [0.4.1](https://github.com/crazyscot/qcp/compare/qcp-v0.4.0...qcp-v0.4.1)

### ⛰️ Features

- Make ANSI colour support optional via config / CLI / environment variables - ([e42f4aa](https://github.com/crazyscot/qcp/commit/e42f4aad91a0db02ba5a42d1733b4ba7a8572f43))
- Send long CLI output to pager (--help, --show-config) - ([b88c49d](https://github.com/crazyscot/qcp/commit/b88c49d171ec953778d05c560104df163ea320d0))
- --show-config also reports validation errors - ([c9fd657](https://github.com/crazyscot/qcp/commit/c9fd657190b8c1dbbea88f2f9cf0038e6778d278))

### 🐛 Bug Fixes

- Correctly apply system default ssh_config and ssh_options ([#113](https://github.com/crazyscot/qcp/pull/113)) - ([0ff032a](https://github.com/crazyscot/qcp/commit/0ff032aac1e5e362d5ac526862128899c4cb486c))
- SshConfig / SshOptions configuration allow a single string ([#113](https://github.com/crazyscot/qcp/pull/113)) - ([d0b5462](https://github.com/crazyscot/qcp/commit/d0b5462b575647a1d7a02edd2dd0c59dca69d787))
- Make SshSubsystem in config files work properly ([#112](https://github.com/crazyscot/qcp/pull/112)) - ([e9f1090](https://github.com/crazyscot/qcp/commit/e9f1090785e206e372ab214521a8ed0672039db1))
- Use anstream::eprintln instead of plain eprintln - ([4530327](https://github.com/crazyscot/qcp/commit/453032739e42cf60e38eda9e2fb8e29bacbfe5b8))

### 🎨 Styling

- _(windows)_ Change table style so it doesn't output mojibake when sent to more - ([65b6a9f](https://github.com/crazyscot/qcp/commit/65b6a9fef74f1363d64868e9efb11eb466d72936))

### 🧪 Testing

- Move unsafe tests out to a separate helper crate - ([e1ae4b2](https://github.com/crazyscot/qcp/commit/e1ae4b2249303a772eeb7f5f39c989a3330cc7f0))
- Various unit tests added ([df7e3a8](https://github.com/crazyscot/qcp/commit/df7e3a85a805dafaa273672502911d2e20093e63)) ([06002d2](https://github.com/crazyscot/qcp/commit/06002d21f62a6ccd2ef023e0eb89192c7f25c55c)) ([d3b79f8](https://github.com/crazyscot/qcp/commit/d3b79f846f14116fc9d4e66c6870001373883847)) ([1e1823f](https://github.com/crazyscot/qcp/commit/1e1823f1ebaf0d6ddd2ab1dc3281c06168169eb0)) ([6fb7a64](https://github.com/crazyscot/qcp/commit/6fb7a64072aa838dd208552d934c9fe3edaf0ad4)) & refactored ([27c5c97](https://github.com/crazyscot/qcp/commit/27c5c97adf103285f07111c4ced30f5923a7cb12))

### 🏗️ Build, packaging & CI

- _(safety)_ Forbid unsafe rust in the qcp main crate - ([dca4765](https://github.com/crazyscot/qcp/commit/dca4765f9572493aa6e54db24d7d008d6b88f689))
- Switch off coveralls - ([946e026](https://github.com/crazyscot/qcp/commit/946e0267093a2a062e4f853d8fcafa2c1b524d25))

### ⚙️ Miscellaneous Tasks

- _(safety)_ Remove unsafe code, add safety policy - ([094193d](https://github.com/crazyscot/qcp/commit/094193ddbc9b79bc49853e2805f6d3fb1c4810d5))
- _(test)_ Fix test leaving stray files in source tree - ([8745dbb](https://github.com/crazyscot/qcp/commit/8745dbb5ba839a9a507addbeb07f4ff63d2117f5))
- Many internal rearrangements for readability and testability
- `LitterTray` is now a separate crate - ([b67d4bb](https://github.com/crazyscot/qcp/commit/b67d4bb6ce75ea538406a66ca80c29b63af08aac))
- Mark some structs and functions as public to support an external testing crate - ([30a24ba](https://github.com/crazyscot/qcp/commit/30a24ba099aa29cb2ca31e6547f08c9880d9a2a6))
- Remove suboptimal error coercion in PortRange - ([a579805](https://github.com/crazyscot/qcp/commit/a5798056d997abe8cc9c29a3b46bd4370d3a0ae3))
- Fix/silence linter warnings for rust 1.87 - ([8aeb3f8](https://github.com/crazyscot/qcp/commit/8aeb3f8ff358862f9387a1eba7fdd1dfb67a5bf7))
- Update manpages, fix garbage - ([0bc9574](https://github.com/crazyscot/qcp/commit/0bc957467b34be5c2745bc2f4d43e3da9fd1fd2d))
- Unify Rust edition 2024 across the workspace - ([73c7249](https://github.com/crazyscot/qcp/commit/73c7249b3209d326c4a8c3f9ee0335c16d53e7e3))
- Fix dead code warnings on windows builds - ([ceeded1](https://github.com/crazyscot/qcp/commit/ceeded17680871442f2ccba5500fcaf49cea5f89))
- Reduce the number of config extractions we perform - ([41b2aad](https://github.com/crazyscot/qcp/commit/41b2aadf43349da5348e0c196de5c2b4af08aa48)) ([d94c3b2](https://github.com/crazyscot/qcp/commit/d94c3b29a60b11ed58368ff8a6995b7abe57fae2))

### 🚜 Refactor

- Configuration validation checks - ([183cd48](https://github.com/crazyscot/qcp/commit/183cd48a5a39515eb37be5a9d2ce3d173ff36ab3))
- Help-buffers mode - ([644bfc5](https://github.com/crazyscot/qcp/commit/644bfc5bccab207e2c0c863f1f493fe091e2efd3))

## [0.4.0](https://github.com/crazyscot/qcp/compare/v0.3.3...v0.4.0)

### ⛰️ Features

- _(config)_ Support ~/.config/qcp/qcp.conf on unix - ([799d2ba](https://github.com/crazyscot/qcp/commit/799d2bae647510b2d79f10ccef538505429da600))
- Add Windows build - ([b4af92a](https://github.com/crazyscot/qcp/commit/b4af92a573237a1e7cbbb18c771ea84a5172df10))
- [**breaking**] Add -l login-name (same short-option as ssh) - ([d7fd7d0](https://github.com/crazyscot/qcp/commit/d7fd7d06de4c584589ba6a0c245a5d843c837062))
- Platform support for OSX and BSD family ([#71](https://github.com/crazyscot/qcp/pull/71)) - ([3302685](https://github.com/crazyscot/qcp/commit/3302685d91515bf4fcdf0ca9e28c860d7ff2a125))
- Introduce --ssh-subsystem mode - ([3faabc5](https://github.com/crazyscot/qcp/commit/3faabc50da159c8a25fdb4a4944a2241c499d7a2))
- Initial-congestion-window can now be specified as an SI quantity (10k, etc) - ([73e085e](https://github.com/crazyscot/qcp/commit/73e085e2a4e2d52a59e16fba4b74a671e4206770))
- Use mimalloc as memory allocator on all builds, in secure mode by default - ([6ec2f99](https://github.com/crazyscot/qcp/commit/6ec2f99e3a0bd474a86afb771e054016e1cc5971))

### 🐛 Bug Fixes

- _(cosmetic)_ Remove struct verbiage from debug output - ([dcfe102](https://github.com/crazyscot/qcp/commit/dcfe10212af093ed22fbd1bb7438287eff56c51c))
- _(cosmetic)_ When compatibility levels are equal, don't say that one is newer - ([5312a3a](https://github.com/crazyscot/qcp/commit/5312a3a75f7d29a36d5f7649c1332df54a41b68f))
- _(protocol)_ Improve reliability of Put pre-transfer check - ([acc3d1a](https://github.com/crazyscot/qcp/commit/acc3d1aa6b4c19722ad66dac59a71d4033c93a7c))
- _(test)_ Occasional random test failure - ([28663a3](https://github.com/crazyscot/qcp/commit/28663a3a5b86c61f1eab538095732c1fa8282c0e))
- _(test)_ Make tracing::setup idempotent - ([92afb90](https://github.com/crazyscot/qcp/commit/92afb9043cccd15655203485f76f421e78341dbb))
- User@host syntax on command line - ([fa0eecd](https://github.com/crazyscot/qcp/commit/fa0eecd0130393d708fb8c646526f4c273dee13c))
- Report i/o errors from Put more reliably - ([783e0d5](https://github.com/crazyscot/qcp/commit/783e0d55dcfcac6a6992832f0dd7cfe4f9b1c954))
- Remove TOCTTOU bug in Put destination checks - ([90977f3](https://github.com/crazyscot/qcp/commit/90977f3eb796d4de8d7c2fc2567bf6e3a86a93f7))

### 📚 Documentation

- Autogenerate part of qcp_config.5; add to xtask man; tweak wording - ([8077399](https://github.com/crazyscot/qcp/commit/80773990884740cc0e8525226237ecb489a32e0f))
- Add note to build in release mode for best performance - ([41ba6d9](https://github.com/crazyscot/qcp/commit/41ba6d977f70cf38966ad051ba07ab580d28aeac))
- Fix broken links in readme on crates.io - ([064a67d](https://github.com/crazyscot/qcp/commit/064a67d5ab1c432f79f4b48dece355330363ecf8))

### ⚡ Performance

- Slight performance improvements to PUT - ([19cfcf9](https://github.com/crazyscot/qcp/commit/19cfcf91432d933329856c15bc1d0c10a0678dbd))

### 🧪 Testing

- _(fix)_ Make test_progress_bar_for() CI-proof - ([e5b3dde](https://github.com/crazyscot/qcp/commit/e5b3ddea3db7c2e3462f4e3be8a6770b81c89ce7))
- Improve unit test coverage in utils - ([3f40c8f](https://github.com/crazyscot/qcp/commit/3f40c8f99d4b7c55ad19077a93101a33f5e6e025))
- Refactor control/process.rs for testability - ([f74870b](https://github.com/crazyscot/qcp/commit/f74870b360e4a8647a702150d52e10eddaf74217))
- Use nightly toolchain for coverage; exclude test modules from analysis ([#43](https://github.com/crazyscot/qcp/pull/43)) - ([ee40c66](https://github.com/crazyscot/qcp/commit/ee40c663af74c1512e6c603c5b1e00bc7a9aebec))
- Introduce LitterTray utility - ([42eec69](https://github.com/crazyscot/qcp/commit/42eec69103332cb7b70fc7ef12b5d783867e985a))

### 🏗️ Build, packaging & CI

- Stop shipping licenses.html - ([c4a62d4](https://github.com/crazyscot/qcp/commit/c4a62d4e461961c699adf772f42c3e8219a42754))
- Pivot windows builds to mingw - ([237ad81](https://github.com/crazyscot/qcp/commit/237ad81da9cfdbd37d82dd3c27918478b2a31942))
- Add codecov reporting - ([935f125](https://github.com/crazyscot/qcp/commit/935f125da218dbb35c07d29adc488530e59f98c7))
- Include a subsystem config for /etc/ssh/sshd_config.d - ([e2780a8](https://github.com/crazyscot/qcp/commit/e2780a880a632274ee4ae8bcba6cad67f5f09b51))
- Improve comments in the default system qcp.conf; add SshSubsystem - ([923af7d](https://github.com/crazyscot/qcp/commit/923af7d527e24d8f3cb8491a2264be834f3a9176))
- Include additional files in the tarballs - ([9ae6bb3](https://github.com/crazyscot/qcp/commit/9ae6bb3836aa54336ef0c4bdd0013bc030b6f6a3))
- Tweak build-time tag version check - ([0bb779f](https://github.com/crazyscot/qcp/commit/0bb779f0bc1f396701a9b434c523690dede68532))

### ⚙️ Miscellaneous Tasks

- Util::open_file returns a less complex error type - ([22f7d07](https://github.com/crazyscot/qcp/commit/22f7d07f61ff0a42d375bcfd1f67a7fa316cb427))
- Autoformat cargo.toml x3 - ([f308eb0](https://github.com/crazyscot/qcp/commit/f308eb01c94269056ee677d628479efdb48b7f02))
- Return type of set_udp_buffer_sizes - ([1358232](https://github.com/crazyscot/qcp/commit/13582325c486e38dcb49e221b816029b67b9d87f))
- Output full error context when we might have one - ([fe875af](https://github.com/crazyscot/qcp/commit/fe875aff918a5f8554caedeefcd07ab26d4262d3))
- Add platform initialiser, seal SocketOptions to ensure it is always called - ([ccf3b49](https://github.com/crazyscot/qcp/commit/ccf3b497f6e89d94c830581c3a8d150904c29e1b))
- Remove user_config_dir from AbstractPlatform - ([f75e199](https://github.com/crazyscot/qcp/commit/f75e199ce9415123a14e5db2d4d81dae420c8895))
- Promote transport::ConfigBucket to crate visibility as config::ConfigProvider - ([232fb75](https://github.com/crazyscot/qcp/commit/232fb75452b413d21efbc05345e1d6cbd0c5a26f))
- Linter fixes for rust 1.86 - ([2dd28cc](https://github.com/crazyscot/qcp/commit/2dd28cc6047c369e1e51b341919b9ceda2d0fcfd))
- Tweak badge config in readme - ([3542a67](https://github.com/crazyscot/qcp/commit/3542a676d2c0b9fd46173cf3e17e593599fd41aa))
- Tidy up crate exports - ([248242e](https://github.com/crazyscot/qcp/commit/248242ed7e738a999d16eb88b3a38eaad6edc7e8))
- Update to rust 2024 / MSRV 1.85 - ([cc6f3e7](https://github.com/crazyscot/qcp/commit/cc6f3e73b0b37f052906e04fbe3d1adea4b2a046))

### 🚜 Refactor

- Use homedir (cross-platform) instead of pwd (works on unix only) - ([b44929f](https://github.com/crazyscot/qcp/commit/b44929f4abe6a7ae0bc2447215bc4b09693ba95e))
- Ssh config file parsing, to allow retrieval of arbitrary keys - ([429a64d](https://github.com/crazyscot/qcp/commit/429a64de225f1b051bdf7bfeb279d610287a7f52))
- Ssh::files::Parser: deduplicate value sources - ([aaf4dc1](https://github.com/crazyscot/qcp/commit/aaf4dc1052d444fd61403f4d160aa8c11e71070e))
- Ssh::files::Parser construction - ([7eb1c74](https://github.com/crazyscot/qcp/commit/7eb1c744dd8aeb4eade551b85a6fdace503b1d94))
- Merge ssh::ConfigFile constructors - ([30dbcef](https://github.com/crazyscot/qcp/commit/30dbcefd32399227790d304c7467caca13c829cd))
- Align Platform return types - ([de7e611](https://github.com/crazyscot/qcp/commit/de7e611aef98fa733286ae6c88634d7c20c3411b))
- Pivot socket back-end to use rustix - ([58d813b](https://github.com/crazyscot/qcp/commit/58d813b4c09b738d7d96b3618ba7410c9c26077f))
- Explicitly pass setup_tracing function to run_server - ([1c12702](https://github.com/crazyscot/qcp/commit/1c12702f8e81343f187d7ba52c692474b940ec23))
- Minimise binary crate main function - ([8a72cf8](https://github.com/crazyscot/qcp/commit/8a72cf8b236b9ad674fa9dc8a5dc5a929b74ded9))
- Consolidate control channel functionality module; add unit tests - ([2a9a356](https://github.com/crazyscot/qcp/commit/2a9a3562f9c4ff2a8a2263ccf465601b5a32a93c))
- Consolidate session protocol implementations; add unit tests - ([71169cc](https://github.com/crazyscot/qcp/commit/71169ccd20b13b781b6930794728001f2b70012f))
- Move CopyJobSpec sharable construction logic into a constructor - ([9535e49](https://github.com/crazyscot/qcp/commit/9535e492ca487e1bbff145842ba3fc5d74b7f5df))
- Move CongestionController(Type) into protocol - ([73b46ec](https://github.com/crazyscot/qcp/commit/73b46ecfcd46b29b676344779a238eee11be71fe))

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

- Client correctly marshals remote_port as None when not specified - ([d1b0054](https://github.com/crazyscot/qcp/commit/d1b0054492ffcf2260c6857b9270d19c5c38fb32))
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

- _(deps)_ Bump rustls from 0.23.16 to 0.23.18 ([#15](https://github.com/crazyscot/qcp/pull/15)) - ([e333abc](https://github.com/crazyscot/qcp/commit/e333abc230528f2172cc2bf9605c5a5b2357d9fc))

### 📚 Documentation

- Add note about build prerequisite - ([6b176c9](https://github.com/crazyscot/qcp/commit/6b176c990d7b29d2dc5af623e3f430c2ee1bdc85))

### ⚙️ Miscellaneous Tasks

- _(build)_ Fix autopublish of Debian packages - ([74b3ea6](https://github.com/crazyscot/qcp/commit/74b3ea6a7be2da3093d4a75a1e92b29946d203ad))

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

- _(ci)_ Align ci and release workflows - ([d16d38a](https://github.com/crazyscot/qcp/commit/d16d38a7aeb7629a76151ae4eb69d1d1b28cd671))
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
