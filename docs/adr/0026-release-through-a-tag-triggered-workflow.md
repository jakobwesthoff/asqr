# 26. Release through a tag-triggered workflow

Date: 2026-10-06

## Status

Accepted

## Context

asqr is published on crates.io, and prebuilt binaries were considered
next to it. The GitHub releases for v0.9.0 to v0.9.2 were created
without a workflow, with the version as title, release notes as body and
no binaries.

## Decision

- `.github/workflows/release.yml` runs on every pushed tag `vX.Y.Z` or
  `vX.Y.Z-rcN`. It creates the GitHub release with
  `taiki-e/create-gh-release-action` and attaches one archive per
  target, `asqr-<tag>-<target>.tar.gz`, built with
  `taiki-e/upload-rust-binary-action`.
- A release's title is the tag and its body is empty.
- Targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`,
  `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl`. There is
  no Windows build, as Windows support is out of scope (spec section 11).
- The actions are pinned to commit SHAs, as in ADR 19.
- No project website yet.

## Consequences

- Publishing to crates.io stays a manual `cargo publish`; the workflow
  only makes the GitHub release.
- The workflow can also be started by hand for a tag that already
  exists, for a tag pushed before the workflow or a run that failed.
