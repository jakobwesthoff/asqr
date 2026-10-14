---
title: "Make the context area a pane of its own with focus and scrolling"
kind: feature
component: tui
status: blocked
horizon: next
origin: discussion
tags: [ux, milestone/v0-11]
---
# Make the context area a pane of its own with focus and scrolling

Step 3 of the context pane plan (design settled with the user on
2026-10-14). The context area from step 2 becomes a pane that shows one
`context` entry (format version 2, ADR 28) with a header, scrolls on
its own, and takes the focus. Images are its first content; the text
and diff viewers of steps 4 and 5 plug into it, replacing the
placeholder that text and diff entries show until then.

## Proposal

The plan's Design section, *Keys*, has the full key set. In short:

- The pane shows the entry's `title` (or file name) in a header line.
- A viewer interface that the image, text and diff viewers implement:
  draw into an area at a scroll offset, report the content's size, and
  say whether sideways scrolling applies. Phase 2 (several items) adds
  a tab strip on top without changing it.
- `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` scroll the pane from anywhere,
  even while typing in a field. `tab` gives the pane the focus, `tab`
  or `esc` gives it back; focused, `↑`/`↓`/`j`/`k` scroll by line,
  `←`/`→`/`h`/`l` sideways, `g`/`G`/`home`/`end` to the ends. No keys
  that need AltGr or Option on non-US layouts.
- `z` shows any entry full screen, `o` opens images as today. Both work
  from the question and from the focused pane.
- On a terminal too small for the pane, it shrinks to a one-line hint
  and `z` shows the entry.
- The key bar and help show the pane's keys while it has the focus.
- Spec section 7.2 and 7.7, and an ADR.

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
- Depends on: [Cap the question at a reading width and pick the pane layout automatically](01M4XHQSBDWD4J2EWY7FB7PKRG-cap-the-question-at-a-reading-width-and-pick-the-pane-layout.md)
