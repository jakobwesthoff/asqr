# 21. Run coverage in CI and on demand only

Date: 2026-09-27

## Status

Accepted

Amends [11. Develop test first with a zero-warning validation pipeline](0011-develop-test-first-with-a-zero-warning-validation-pipeline.md)

## Context

ADR 11 put `cargo llvm-cov` into `just check`, which the pre-commit hook
runs. The adversarial review (F7) found three problems:

- The hook ran every test twice, once plain and once instrumented.
- The coverage build compiles the whole dependency tree a second time,
  which makes the first commit after a `cargo add` slow.
- The coverage number is only ever read in review.

The reviewer and I agreed on replacing `cargo test` with `cargo llvm-cov`
so tests run once. Whether coverage stays in the hook at all was left to
the user.

## Decision

Coverage leaves the hook (user's decision).

- `just check`, which the hook runs, is license notices, `cargo fmt
  --check`, clippy with `-D warnings`, `cargo test --all-targets` and
  `cargo test --doc`.
- `just coverage` runs `cargo llvm-cov` on demand.
- GitHub Actions runs both.
- Coverage is still reported without a threshold, and review checks that
  every written or changed line is covered.

The same review also brought:

- `cargo test --doc`, because `--all-targets` skips doctests
- a `just setup` that checks for `just`, `cargo-llvm-cov` and
  `llvm-tools-preview`
- a note in the hook that it checks the working tree, not the index

## Consequences

Commits stay fast when new crates arrive. The coverage number only shows
up after a push (CI) or when someone runs `just coverage`.
