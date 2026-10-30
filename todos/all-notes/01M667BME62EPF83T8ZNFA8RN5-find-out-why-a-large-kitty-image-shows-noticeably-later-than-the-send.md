---
title: "Find out why a large Kitty image shows noticeably later than the send time says"
kind: investigation
component: tui
origin: discussion
tags: [ux, field-test]
---
# Find out why a large Kitty image shows noticeably later than the send time says

Jakob sees large Kitty images appear noticeably later than small ones,
while the timings asqr can measure do not show a matching difference.
He asked for this todo on 2026-10-30 (asqr session
01M666TG80F3WXVV7K8P1PW3PH) after a measured screenshot run.

## What was observed

- Earlier, in a field test: ten 768 px option images felt slower than
  one 6000×6000 image (recorded in the todo about sending Kitty images
  at their own size).
- 2026-10-30, Ghostty 1.3.2, 177×69 cells of 15×39 px, with a demo that
  sends one image per step and measures the write and the time until
  the terminal answers a Kitty query (`a=q`) sent after the image:
  - asqr scales the picture to its cells (about 34 MB of base64 for a
    full pane): directly 120 to 220 ms, inside tmux 3.7c 0.7 to 2 s;
  - the terminal scales a 512 px picture (1.45 MB): 6 ms directly,
    33 ms inside tmux.
  - Jakob's note: "I am fairly sure that showing the big pictures here
    felt noticeably slower than the smaller ones that were scaled
    automatically. I am not sure that what you measure is the same as
    what the actual render cycle costs afterwards."
- Screenshots and numbers are in the conductor logbook of 2026-10-30,
  `~/playground/2026_10_30_asqr-todos-worked-off-by-agents/plans/kitty/`.

## Open questions

- Does the terminal answer the query before it has decoded, uploaded and
  drawn the picture? Then the measured time ends too early, and the
  felt delay is the terminal's own work on 34 MB.
- Do several images at once cost more than their bytes (repeated
  layout, deletes and re-sends, tmux passthrough queuing)?
- How can the delay be measured from outside, for example with a clock
  drawn next to the image and caught on screen recordings?

## Relations

- Relates to: [Send Kitty images at their own size and let the terminal scale them](01M520Z555XDCXKXYHFXSNNBEK-send-kitty-images-at-their-own-size-and-let-the-terminal-scale-them.md)
