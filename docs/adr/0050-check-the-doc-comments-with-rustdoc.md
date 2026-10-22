# 50. Check the doc comments with rustdoc

Date: 2026-10-22

## Status

Accepted

Amends [11. Develop test first with a zero-warning validation pipeline](0011-develop-test-first-with-a-zero-warning-validation-pipeline.md)

## Context

`just check` did not run rustdoc. Four doc comments that rustdoc
rejects under `-D warnings` were on `main`: three public items linked
to private ones, and a `<DIR>` in running text read as an unclosed HTML
tag.

## Decision

`just check` also runs

    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items

so a broken doc link fails the check. The pre-commit hook and CI run it
through `just check`.

## Consequences

The step documents the library only. The binary has the library's name,
so `cargo doc` leaves it out, and the doc comments in `src/main.rs` and
`src/terminal.rs` are not checked.
