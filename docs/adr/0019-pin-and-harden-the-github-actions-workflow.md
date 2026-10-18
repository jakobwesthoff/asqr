# 19. Pin and harden the GitHub Actions workflow

Date: 2026-09-27

## Status

Accepted

## Context

`actions/checkout` was two majors behind, and the workflow followed
none of the hardening advice in GitHub's security guide.

## Decision

- Actions are pinned to full commit SHAs with the version in a comment.
  `.github/dependabot.yml` keeps the pins and their version comments
  current, weekly, together with the Cargo dependencies.
- `dtolnay/rust-toolchain` has no release tags. It is pinned at the head
  of its `stable` branch with `toolchain: stable`, and that pin is bumped
  by hand, as Dependabot cannot follow a branch.
- `permissions: contents: read` for the whole workflow.
- `persist-credentials: false` on checkout.
- `concurrency` cancels superseded runs.
- `CARGO_TERM_COLOR=always`.

Kept: `dtolnay/rust-toolchain` with `Swatinem/rust-cache`. GitHub code
search on 2026-09-27 counted about 196,000 workflow files using it,
against about 14,000 for `actions-rust-lang/setup-rust-toolchain`.
Kept as well: `ubuntu-latest` and `macos-latest`, although
`ubuntu-latest` moves to Ubuntu 26.04 in November 2026
(actions/runner-images#14748).

## Consequences

The switch of `ubuntu-latest` to 26.04 will reach CI without a change
in the repo.
