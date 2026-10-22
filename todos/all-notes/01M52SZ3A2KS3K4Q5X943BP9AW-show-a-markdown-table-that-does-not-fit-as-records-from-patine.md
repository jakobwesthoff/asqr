---
title: "Show a Markdown table that does not fit as records from patine"
kind: bug
component: tui
status: blocked
origin: review
tags: [ux]
---
# Show a Markdown table that does not fit as records from patine

Found in the review of the patine 1.7.1 pickup on 2026-10-16. A
Markdown table with more columns than its width holds is unreadable in
asqr: patine shrinks its columns to single letters, and below one
character per column the table overflows. asqr's adapter
(`src/tui/render/patine_adapter.rs`, `indent`) then wraps each row and
repeats its border `│ ` as if it were a quote bar. A row with a
halfwidth sound mark such as `ﾞ` overflows too, as patine counts the
mark as no column and asqr as one (ADR 0033).

## Problem

Reproduced on 2026-10-16 with patine 1.7.2 through the Markdown viewer
(`src/tui/render/markdown_viewer.rs`): twelve one-letter columns need 49
columns, so at a width of 20 every border and row wraps; a cell of `ｶﾞ`
six times at a width of 12 draws `│ x │ ｶﾞｶﾞｶﾞ` and then `│ ↪ ｶﾞ │`.
The test `a_table_row_patine_leaves_wider_still_wraps` covers both.
Rendered with `patine -w 22`, eight columns of short words already
break into one letter per line.

## Proposal

patine lays out a table as records once its columns would break words,
and counts a halfwidth sound mark as a column of its own (the two patine
todos below). asqr then picks up that patine release:

- Rewrite `a_table_row_patine_leaves_wider_still_wraps` for records and
  for the sound mark fitting, test first.
- Spec section 7.10: a table that does not fit shows as records; drop
  the table and the sound mark from the list of lines that wrap.
- ADR 0034, which names the remaining wrap cases, through a new ADR
  that amends it.
- `CHANGELOG.md`: the patine version and the records layout.

## Decisions

- 2026-10-16, user: patine breaks a word longer than its table column
  across lines, so a table stays whole and fits (patine 1.7.2, ADR 0034).
- 2026-10-22, user: a table that does not fit shows as records, one
  line per column, over wrapping its rows without the false quote bar
  and over cutting it with sideways scrolling.
- 2026-10-22, user: records as soon as a column would break words, not
  only once the table overflows.
- 2026-10-22, user: patine counts a halfwidth sound mark as a column of
  its own; asqr does not correct it.

## Relations

- Depends on: patine todo 01M5HRF9S39JGVKFBV8EVYVFQQ (Lay out a table as records when its columns would break words)
- Depends on: patine todo 01M5HRF9SXAT2T7VBJC7Z7MG8A (Count a halfwidth sound mark as a column of its own)
