---
title: "Make the context area a pane of its own with focus and scrolling"
kind: feature
component: tui
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

## Where step 2 left it

Step 2 (ADR 30) placed the context area. `src/tui/render/panes.rs`
decides beside, below or a one-line hint (`choose`) and the areas
(`areas`); `draw_tab` and `draw_context` in `src/tui/render/screen.rs`
draw into them. The hint (`context_hint`) offers `z` for images only and
carries a TODO for this step. The intro overlay (`i`, in `App`) already
scrolls with `pgup`/`pgdn`, `ctrl-u`/`ctrl-d`, `↑`/`↓`, `j`/`k`,
`g`/`G` and `home`/`end`; while it is open it takes every key, so the
pane's keys do not reach the pane then.

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
