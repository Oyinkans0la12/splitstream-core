# Contributing

Build, test, coding-standards, and PR expectations live in this repo's
[CONTRIBUTING.md](../CONTRIBUTING.md). Read that first; nothing here replaces
it.

## Where this repo sits in the cross-repo contract

This repo is the settlement layer. [splitstream-actions] computes what gets
settled — the points, the manifest, the Merkle root — and
[splitstream-sdk-cli] is how people interact with the result: contributors claim
with it, maintainers simulate and report with it. Both of them are clients of
this contract's interface, so a change here that touches the Merkle leaf format
or the shape of the manifest this contract expects is a **breaking cross-repo
change**, not a local refactor. The leaf format is frozen
(`sha256(contributor.to_xdr ++ amount.to_xdr)`, sorted-pair siblings); changing
it silently invalidates every proof splitstream-actions produces, and every
claim fails with `InvalidProof` until that repo ships a matching change. The same
applies to the error codes (they are on-chain ABI) and to any change in when a
root becomes final.

Before changing anything on that boundary, read what depends on it:

- [splitstream-actions docs](https://splitstream.gitbook.io/splitstream-actions/) — manifest generation and root relay.
- [splitstream-sdk-cli docs](https://splitstream.gitbook.io/splitstream-sdk-cli/) — claim flow and the CLI's contract interface.

A change that cannot avoid the boundary needs the other two repos updated in
step with it; state that dependency explicitly in the PR description.

## Git workflow

One commit per logical unit, conventional commit messages
(`type(scope): description`), stage specific files rather than `git add .` —
all per [CONTRIBUTING.md](../CONTRIBUTING.md).

`main` is protected by the repository ruleset `main-protection`, so a direct
push will not land. The ruleset requires:

- all changes through a **pull request** (0 approvals required, but review
  threads must be resolved);
- the **`ci` status check** to pass, and the branch to be up to date with `main`;
- **squash merges only**, with linear history; force-pushes and deletions of
  `main` are blocked.

So the working pattern is: commit locally, push the commit to a branch, open a
Pull Request against `main`, and let CI run. Because only squash merges are
allowed, a PR with several commits lands as one commit — keep one logical unit
per PR if you want one commit per change in `main`'s history.

[splitstream-actions]: https://splitstream.gitbook.io/splitstream-actions/
[splitstream-sdk-cli]: https://splitstream.gitbook.io/splitstream-sdk-cli/
