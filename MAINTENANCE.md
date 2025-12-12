## Development notes

### rust-analyzer (VS Code) issues

The rust-analyzer plugin for Visual Studio Code comes bundled with a specific version of rust-analyzer.

As this project does not automatically track the latest MSRV, sometimes the newer analyzer doesn't quite work properly.

The recommended fix for this is:

- rustup component add rust-analyzer
- Add this to `~/.config/Code/User/settings.json:`:
  - `"rust-analyzer.server.path": "${userHome}/.cargo/bin/rust-analyzer"`

## Releases

### Prerequisites

- `git config tag.gpgsign true`
- Consider setting this `--global`
- Consider also `git config commit.gpgsign true`

### Tooling

- `cargo install --locked release-plz cargo-semver-checks`

### Creating a release

- Check and update dependencies as required.
- `cargo semver-checks` will tell you whether there are any breaking API changes that prompt a version bump.
- Confirm top-level docs have been updated for any changes since the last release.
- Update SECURITY.md if this is a new major or minor release.
- Update the News section in README.md if appropriate.
- Update the template `qcp.conf` if any options have been added.
- Update man page & packaged HTML docs if required:
  - `cargo xtask man && cargo xtask clidoc`
    - _N.B. This isn't automated in CI to save repeating work across multiple builds._
  - Commit it
- Create PR:
  - _(Optional)_ `release-plz update` to preview updates to the changelog and version
  - `release-plz release-pr --git-token $GITHUB_QCP_TOKEN`
  - _if this token has expired, you'll need to generate a fresh one; walk back through the release-plz setup steps_
- Review changelog, edit if necessary.
- _(Optional)_ `release-plz set-version qcp@<version>` to override the automatic version rules
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
      - `nix develop ./nix`
      - `cd nix && nix build .`
      - Edit or skip tests as appropriate, as not all of them will be able to succeed in the nix sandbox.

### Adding sub-crates

If you add a new crate that should not be published to crates.io, ensure it's marked
as `release = false` in release-plz.toml.

If we later move to multiple published crates from this repo, we will need to update
the version string logic in `qcp/build.rs`.
