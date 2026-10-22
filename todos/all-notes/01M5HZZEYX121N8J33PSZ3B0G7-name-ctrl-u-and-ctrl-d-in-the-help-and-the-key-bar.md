---
title: "Name ctrl-u and ctrl-d in the help and the key bar"
kind: improvement
component: tui
origin: request
tags: [ux]
---
# Name ctrl-u and ctrl-d in the help and the key bar

Raised on 2026-10-22 while testing asqr 0.13.0: `ctrl-u` and `ctrl-d`
scroll half a page, but the key bar never names them, and the general
help (`?`) says `pgup/pgdn  scroll the context pane, also ctrl-u/ctrl-d`
(`HELP` in `src/tui/render/keys.rs`), which reads as if they moved the
same distance as `pgup`/`pgdn`. Only the help of the focused pane and of
the full-screen view lists them on a line of their own.

## Proposal

- General help: the one line names both distances, for example
  `pgup/pgdn, ctrl-u/ctrl-d  scroll the pane by a page, half a page`.
  The general help keeps its 13 lines, which fill the smallest screen
  (ADR 31).
- Key bar while the pane or the full-screen view has the focus: a hint
  for `ctrl-u/ctrl-d` beside `pgup/pgdn  page`.
- Key bar while a pane shows but does not have the focus: a hint for
  `ctrl-u/ctrl-d` too.
- Both new hints rank low, so they leave the bar first where it is
  narrow (the key bar's ranking, ADR 31).

## Decisions

- 2026-10-22, user: all three places: the general help, the key bar with
  the pane focused, and the key bar while a pane shows unfocused.
- 2026-10-22, user: implemented right away and released as 0.13.1.
