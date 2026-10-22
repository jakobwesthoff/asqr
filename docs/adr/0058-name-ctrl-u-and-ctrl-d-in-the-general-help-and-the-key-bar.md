# 58. Name ctrl-u and ctrl-d in the general help and the key bar

Date: 2026-10-22

## Status

Accepted

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amends [52. Scroll half a page with ctrl-u and ctrl-d](0052-scroll-half-a-page-with-ctrl-u-and-ctrl-d.md)

## Context

`ctrl-u` and `ctrl-d` scroll half a page, `pgup` and `pgdn` a whole one
(ADR 52). The general help named them on the line of `pgup`/`pgdn`, as
`pgup/pgdn  scroll the context pane, also ctrl-u/ctrl-d`, which reads as
if they moved as far as `pgup` and `pgdn`. The key bar never named them;
only the helps of the focused pane and of the full-screen view listed
them on a line of their own.

The general help has 13 lines, which with its frame fill the 15 rows of
the smallest screen (ADR 31). Its frame's height is set before the lines
wrap, so a line that wraps pushes the last one out of the frame. On the
smallest screen, 60 columns wide, 56 are left inside the frame; the
longest description, `toggle an option; pick or toggle one directly`,
takes 45 of them after a key column of 11.

## Decision

- **General help:** one line names both keys and how far each scrolls:
  `pgup/pgdn, ctrl-u/ctrl-d  scroll by page; by half a page`. The general
  help keeps its 13 lines.
- **Key column:** a help's key column is as wide as its longest key and
  a blank, and at least 11 columns, but no wider than leaves its longest
  description one line on the smallest screen. A key longer than the
  column is followed by one blank and pushes its description along. The
  general help's column stays at 11, and its scroll line's key runs past
  it; the helps of the pane and of the full-screen view keep their
  column of 14.
- **Key bar:** `ctrl-u/ctrl-d  half page` follows `pgup/pgdn  page`
  while the pane or the full-screen view has the focus, and follows
  `tab  context` while a pane shows without the focus, in a field and a
  note too.
- **Rank:** the hint ranks below `n`, so a key bar too narrow for every
  hint leaves it out first.
- In ADR 31 this changes the key bar's order of leaving out hints, which
  starts with `ctrl-u`/`ctrl-d`, before `n`. In ADR 52 it replaces the
  general help's line `pgup/pgdn  scroll the context pane, also
  ctrl-u/ctrl-d`.

## Consequences

- On 120 columns the key bar of a `single` question with a note beside
  a pane shows `ctrl-u/ctrl-d  half page`; on 100 columns it leaves it
  out and keeps `n  note`.
