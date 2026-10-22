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
again. Vim's `ctrl-f` keeps two lines of overlap. Split off on
2026-10-22 from the todo that makes `ctrl-u` and `ctrl-d` move half a
page.

## Proposal

`pgdn` and `pgup` move by the view's height less a few lines, so the
last lines before the jump show again at the other end.

## Open questions

- How many lines of overlap, and what a view of only a few rows does.
- Whether the help, the spec and ADR 30 and ADR 31 name the overlap.

## Relations

- Relates to: [Scroll half a page with ctrl-u and ctrl-d as Vim does](01M5A4TE7V9S2Y46V9V75VB8QV-scroll-half-a-page-with-ctrl-u-and-ctrl-d-as-vim-does.md), changes the same `scroll` function
