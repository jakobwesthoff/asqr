# 11. Develop test first with a zero-warning validation pipeline

Date: 2026-09-27

## Status

Accepted

## Context

The tool is developed test driven from the first line, with no clippy
warnings and good test coverage for everything written or changed.

Coverage in the pre-commit hook has three problems:

- The hook would run every test twice, once plain and once instrumented.
- The coverage build compiles the whole dependency tree a second time,
  which makes the first commit after a `cargo add` slow.
- The coverage number is only ever read in review.

## Decision

- **Test first:** write the test, watch it fail, write the feature, watch
  it pass, then move on to the next one.
- **The check:** the recipe `just check` runs
  - the check of the license notices
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test --all-targets`
  - `cargo test --doc`, because `--all-targets` skips doctests
- **Pre-commit hook:** a hook versioned in the repo runs `just check`, so
  nothing red gets committed. The hook notes that it checks the working
  tree, not the index.
- **Coverage:** `just coverage` runs `cargo llvm-cov` on demand, outside
  the hook. It is reported only, with no threshold. Every code change is
  expected to be covered, and review checks it.
- **CI:** GitHub Actions runs `just check` and `just coverage`.
- **Setup:** `just setup` enables the hook and checks for `just`,
  `cargo-llvm-cov` and `llvm-tools-preview`.
- **TUI tests:** go through the application state and ratatui's
  `TestBackend`, with `insta` snapshots.

## Consequences

Each commit carries its tests. Terminal setup and teardown, which cannot
run in tests, stay as thin as possible. Commits stay fast when new
crates arrive. The coverage number only shows up in CI or when someone
runs `just coverage`.
