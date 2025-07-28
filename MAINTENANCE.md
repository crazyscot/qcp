## Prerequisites

- `git config tag.gpgsign true`
- Consider setting this `--global`
- Consider also `git config commit.gpgsign true`

## Creating a release

- Check and update dependencies as required.
- Confirm top-level docs have been updated for any changes since the last release.
- Update SECURITY.md if this is a new major or minor release.
- Update the News section in README.md if appropriate.
- Update man page if required:
  - `cargo xtask man`
    - _N.B. This isn't automated in CI to save repeating work across multiple builds._
  - Commit it
- Create PR:
  - _(Optional)_ `release-plz update` to preview updates to the changelog and version
  - `release-plz release-pr --git-token $GITHUB_QCP_TOKEN`
  - _if this token has expired, you'll need to generate a fresh one; walk back through the release-plz setup steps_
- Review changelog, edit if necessary.
- Merge the PR (rebase strategy preferred)
- Delete the PR branch
- `git fetch && git merge --ff-only`
- Finalise the release:
  - `release-plz release --git-token $GITHUB_QCP_TOKEN`
  - Check the new Github release page; update notes as necessary. Publication of the github release triggers the artifact builds.
- Merge `dev` into `main`, or whatever suits the current branching strategy
  - main is set to require linear history and is a protected branch. **Do not rebase-merge!**
    - Create a PR for dev into main in the usual way.
    - Locally make the fast-forward merge
    - Push to main. Even though it is protected the PR is allowed.
- Check the docs built, follow up on the release workflow, etc.
- Update the build support files.
  - For Nix packages:
    - Update tagged release version in `nix/default.nix`
    - Pre-compute the release hash (v0.3.0 is the release in this example) and replace the `fetchFromGithub.hash` value to the output SRI: `nix hash to-sri --type sha256 "$(nix-prefetch-url --unpack 'https://github.com/crazyscot/qcp/archive/v0.3.0.tar.gz')"`.
    - Run a test build with previous hashes or `lib.fakeHash` set for `cargoHash` and place the expected hash in place.

## Adding sub-crates

If you add a new crate that should not be published to crates.io, ensure it's marked
as `release = false` in release-plz.toml.

If we later move to multiple published crates from this repo, we will need to update
the version string logic in `qcp/build.rs`.
