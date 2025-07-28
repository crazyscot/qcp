# Contributing to QCP

## 🐛 Bug reports & Feature requests

Bug reports and feature requests are welcome, please open an [issue].

- It may be useful to check the [issues list] and the [discussions] first in case somebody else has already raised it.
- Please be aware that I mostly work on this project in my own time.

## 🏗️ Pull request policy

If you're thinking of contributing something non-trivial, it might be best to raise it in [discussions] first so you can get feedback early. This is particularly important for new features, to ensure they are aligned with the project goals and your approach is suitable.

- Changes should normally be based on the `dev` branch. _(Exception: hotfixes may be branched against `main`.)_
- PRs must pass the full set of CI checks (see below). No exceptions.
- Unit tests are encouraged, particularly those which fail before and pass after a fix.
  - The CI workflow runs code coverage analysis.
- Refactoring for its own sake is OK if driven by a feature or bugfix.
- Clean commit histories are preferred, but don't be discouraged if you don't know how to do this. git can be a tricky tool.
- Commit messages should follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
  - Begin the commit message for a feature with `feat:`. If it's a bugfix, use `fix:`.
  - For a full list of supported message tags, look at `commit_parsers` in [release-plz.toml](release-plz.toml).
  - This policy is in force since release 0.1.0. Earlier commits are considered grandfathered in.
- Where there is an issue number, commit messages should reference it, e.g. (#12)
- Do not edit CHANGELOG.md, that will be done for you on release.

## ☑️ CI checks applied

| Check                 | How to run it yourself                                    | Notes                                                                     |
| --------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------- |
| Code style            | `cargo fmt --all --check`                                 | For VS Code users, `editor.formatOnSave=true` is set                      |
| Everything must build | `cargo build --all-targets`                               |
| Unit tests pass       | `cargo test`                                              |
| Lints                 | `cargo clippy --all-targets`                              | This is a reasonably pedantic set of lints, which I make no apologies for |
| Docs build            | `cargo doc --no-deps`                                     |
| Code coverage         | `./scripts/coverage`, then examine `target/coverage/html` |

[issue]: https://github.com/crazyscot/qcp/issues/new/choose
[issues list]: https://github.com/crazyscot/qcp/issues
[discussions]: https://github.com/crazyscot/qcp/discussions
