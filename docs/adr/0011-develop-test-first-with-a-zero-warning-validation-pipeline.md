# 11. Develop test first with a zero-warning validation pipeline

Date: 2026-09-27

## Status

Accepted

## Context

The user wants the tool developed test driven from the first line, with
no clippy warnings and good test coverage for everything written or
changed.

## Decision

- **Test first:** write the test, watch it fail, write the feature, watch
  it pass, then move on to the next one.
- **The check:** the recipe `just check` runs
  - `cargo fmt --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test`
  - `cargo llvm-cov`
- **Pre-commit hook:** a hook versioned in the repo runs `just check`, so
  nothing red gets committed. GitHub Actions follows once the repo has a
  remote.
- **Coverage:** reported only, with no threshold (user's choice). Every
  code change is expected to be covered, and review checks it.
- **TUI tests:** go through the application state and ratatui's
  `TestBackend`, with `insta` snapshots.

## Consequences

Each commit carries its tests. Terminal setup and teardown, which cannot
run in tests, stay as thin as possible.
