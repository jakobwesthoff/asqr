---
title: "Keep a few lines of overlap when pgdn and pgup scroll a page"
kind: improvement
component: tui
origin: discussion
tags: [ux]
---
# Keep a few lines of overlap when pgdn and pgup scroll a page

`pgup` and `pgdn` scroll the context pane, the full-screen view and the
intro overlay by exactly one view height (`scroll` in
`src/tui/scroll.rs`). The last line shown before `pgdn` is gone after
it, so the reader has no line to hold on to and has to find the place
again. Vim's `ctrl-f` keeps two lines of overlap. The same function
moves `ctrl-u` and `ctrl-d` by half the view's height.

## Proposal

`pgdn` and `pgup` move by the view's height less a few lines, so the
last lines before the jump show again at the other end.

## Open questions

- How many lines of overlap, and what a view of only a few rows does.
- Whether the help, the spec and ADR 30, ADR 31 and ADR 52 name the
  overlap.
