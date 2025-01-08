## Creating a release

* Create PR:
  * _(Optional)_ `release-plz update` to preview updates to the changelog and version
  * ```release-plz release-pr --git-token $GITHUB_QCP_TOKEN```
  * _if this token has expired, you'll need to generate a fresh one; walk back through the release-plz setup steps_
* Review changelog, edit if necessary.
* Merge the PR (rebase strategy preferred)
* Delete the PR branch
* `git fetch && git merge --ff-only`
* Finalise the release:
  * ```release-plz release --git-token $GITHUB_QCP_TOKEN```
  * Check the new Github release page; update notes as necessary. Publication of the github release triggers the artifact builds.
* Merge `dev` into `main`, or whatever suits the current branching strategy
  * main is set to require linear history, which will often mean a rebase-merge and a fresh `dev` branch.
* Check the docs built, follow up on the release workflow, etc.
