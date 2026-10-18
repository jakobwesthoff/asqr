# 26. Release through a tag-triggered workflow

Date: 2026-10-06

## Status

Accepted

Supersedes [20. Use asqr locally before publishing it](0020-use-asqr-locally-before-publishing-it.md)

## Context

ADR 20 left the decision to publish open and considered prebuilt
binaries next to crates.io. asqr is published: versions 0.9.0 to 0.10.0
are on crates.io. The GitHub releases for v0.9.0 to v0.9.2 were created
without a workflow, with the version as title, release notes as
body and no binaries.

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
