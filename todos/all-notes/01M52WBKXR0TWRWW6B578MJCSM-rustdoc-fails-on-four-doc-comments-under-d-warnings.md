---
title: "Rustdoc fails on four doc comments under -D warnings"
kind: chore
component: project
impact: low
origin: implementation
---
# Rustdoc fails on four doc comments under -D warnings

`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items`
stops with four errors on asqr main a584e68 (2026-10-16). `just check`
never runs rustdoc, so nothing catches them. Found by the agent that
picked up patine 1.7.2 and reproduced by hand.

## Problem

- `src/format/warnings.rs:43`: the public docs of `LargeEntry` link to
  the private `LARGE_ENTRY_BYTES`.
- `src/tui/render/screen.rs:246`: the public docs of `draw` link to the
  private `draw_screen`.
- `src/tui/text_field.rs:242`: the public docs of `paste` link to the
  private `cleaned`.
- `src/cli/mod.rs:160`: an unclosed HTML tag `DIR`, where `<DIR>` is
  meant as text and needs backticks.

## Suggested fix

Fix the four comments: state the private item's fact in words or make
the link target visible, and put `<DIR>` in backticks. Then add
`RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items`
to `just check`, so a broken doc link fails the check.

Still failing on main `f9c5b86` (2026-10-22).

## Decisions

- 2026-10-22, user: `just check` runs rustdoc with `-D warnings`.
