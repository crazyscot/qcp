# Contributing to QCP

## 🐛 Bug reports & Feature requests

Bug reports and feature requests are welcome, please open an [issue].

- It may be useful to check the [issues list] and the [discussions] first in case somebody else has already raised it.
- Please be aware that I mostly work on this project in my own time.

## 🏗️ Pull request policy

If you're thinking of contributing something non-trivial, it might be best to raise it in [discussions] first so you can get feedback early. This is particularly important for new features, to ensure they are aligned with the project goals and your approach is suitable.

* Changes should normally be based on the `dev` branch. _(Exception: hotfixes may be branched against `main`.)_
* PRs must pass CI. No exceptions.
* Unit tests are encouraged, particularly those which fail before and pass after a fix.
* Refactoring for its own sake is OK if driven by a feature or bugfix.
* Clean commit histories are preferred, but don't be discouraged if you don't know how to do this. git can be a tricky tool.
* Commit messages should follow [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).
  * Begin the commit message for a feature with `feat:`. If it's a bugfix, use `fix:`. 
  * For a full list of supported message tags, look at `commit_parsers` in [release-plz.toml](release-plz.toml).
  * This policy is in force since release 0.1.0. Earlier commits are considered grandfathered in.
* Where there is an issue number, commit messages should reference it, e.g. (#12)
* Do not edit CHANGELOG.md, that will be done for you on release.

[issue]: https://github.com/crazyscot/qcp/issues/new/choose
[issues list]: https://github.com/crazyscot/qcp/issues
[discussions]: https://github.com/crazyscot/qcp/discussions
