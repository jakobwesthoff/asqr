---
title: "Decide whether the reading width can be set per terminal"
kind: investigation
component: tui
status: needs-discussion
origin: request
tags: [ux]
---
# Decide whether the reading width can be set per terminal

The user asked on 2026-10-15 whether the width at which the question
side is cut off should be overridable through an environment variable
or a command line flag, for terminals that differ in size, font or
use.

## Context

- `READING_WIDTH` in `src/tui/render/panes.rs` caps the question at 100
  columns. Its comment records the reason: on a terminal of 230 columns
  question texts ran across all of it, and 100 was the first value
  tried from a planned 100 to 110 (ADR 0030).
- The same file fixes `MIN_QUESTION_WIDTH` (60) and `MIN_CONTEXT_WIDTH`
  (40), which decide when context fits beside the question.
- A precedent for such a setting exists: `--layout auto|beside|below`
  and `ASQR_LAYOUT` choose where context goes (`src/cli/mod.rs`).

## Options

- Keep 100 fixed.
- A flag and a variable for the reading width only, for example
  `--reading-width <columns>` and `ASQR_READING_WIDTH`, checked only
  when asqr watches, like `--layout`.
- Also expose the two minimum widths. More knobs, and they interact
  with the beside/below decision.

## Open questions

- Which terminal variations make 100 wrong in practice: a very wide
  monitor, a large font, a narrow split?
- Limits for the value: at least the 60 columns asqr needs, at most the
  terminal's width?
- Does a changed reading width move the point where context goes
  beside the question (ADR 0030), and should it?
