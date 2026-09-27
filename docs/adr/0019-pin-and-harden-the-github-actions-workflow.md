# 19. Pin and harden the GitHub Actions workflow

Date: 2026-09-27

## Status

Accepted

## Context

A review on 2026-09-27 found `actions/checkout` two majors behind (v5,
latest v7.0.1). It also found that the workflow followed none of the
hardening advice in GitHub's security guide.

## Decision

- Actions are pinned to full commit SHAs with the version in a comment,
  resolved with `gh api`:
  - `actions/checkout` v7.0.1
  - `taiki-e/install-action` v2.87.21
  - `Swatinem/rust-cache` v2.9.2
  - `dtolnay/rust-toolchain`, pinned at the head of its `stable` branch
    with `toolchain: stable`, because it has no release tags
- `permissions: contents: read` for the whole workflow.
- `persist-credentials: false` on checkout.
- `concurrency` cancels superseded runs.
- `CARGO_TERM_COLOR=always`.
- `.github/dependabot.yml` updates the actions and the Cargo
  dependencies weekly.

Kept: `dtolnay/rust-toolchain` with `Swatinem/rust-cache`. GitHub code
search on 2026-09-27 counted about 196,000 workflow files using it,
against about 14,000 for `actions-rust-lang/setup-rust-toolchain`.
Kept as well: `ubuntu-latest` and `macos-latest`, although
`ubuntu-latest` moves to Ubuntu 26.04 in November 2026
(actions/runner-images#14748).

## Consequences

Dependabot cannot follow the `rust-toolchain` branch, so that pin is
bumped by hand. The switch of `ubuntu-latest` to 26.04 will reach CI
without a change in the repo.
