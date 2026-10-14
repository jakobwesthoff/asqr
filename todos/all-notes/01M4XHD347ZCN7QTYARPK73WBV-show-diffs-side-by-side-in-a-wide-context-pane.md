---
title: "Show diffs side by side in a wide context pane"
kind: feature
component: tui
status: blocked
origin: discussion
tags: [ux]
---
# Show diffs side by side in a wide context pane

Decided with the user on 2026-10-14 (asqr session
`01M4XGMR0HK8KKZYHH9SF3NSTD`) while designing the context pane. Its
first phase renders a diff item as a unified diff. In a pane wide
enough for two columns of code, about 160 columns and more, the old
text goes on the left and the new text on the right instead, with
changed lines aligned.

## Proposal

- The diff item already carries old and new text (format version 2), so
  no format change is needed: asqr lays out the same hunks in two
  columns.
- Unified below the width threshold, side by side above it; the
  threshold is tuned by trying it.
- Hunks, context lines, line numbers and word emphasis work as in the
  unified view.

## Open questions

- Whether the person can switch between the two views with a key.
- How removed and added runs of different lengths are aligned (blank
  filler rows on the shorter side).

## Relations

- Depends on: [Show diff entries in the context pane as a unified diff](01M4XHQSDRCG7M60ZQ5M2D530J-show-diff-entries-in-the-context-pane-as-a-unified-diff.md)
