---
title: "Find out why a large Kitty image shows noticeably later than the send time says"
kind: investigation
component: tui
status: needs-discussion
horizon: someday
origin: discussion
tags: [ux, field-test]
---
# Find out why a large Kitty image shows noticeably later than the send time says

Jakob sees large Kitty images appear noticeably later than small ones,
while the timings asqr can measure do not show a matching difference.
He asked for this todo on 2026-10-30 (asqr session
01M666TG80F3WXVV7K8P1PW3PH) after a measured screenshot run.

## What was observed

- Earlier, in a field test with a release build of `f5f26a3`: ten
  768 px option images felt slower than one 6000×6000 image, although
  both went out raw at 2220×2223 px.
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
- Since ADR 80, asqr sends a Kitty image it enlarges 3 times or more
  (with cells of 15×39) at a third of its cells' pixels, a ninth of the
  bytes, and the terminal enlarges it the rest of the way. How that
  changes the felt delay is not measured.

## Open questions

- Does the terminal answer the query before it has decoded, uploaded and
  drawn the picture? Then the measured time ends too early, and the
  felt delay is the terminal's own work on 34 MB.
- Do several images at once cost more than their bytes (repeated
  layout, deletes and re-sends, tmux passthrough queuing)?
- How can the delay be measured from outside, for example with a clock
  drawn next to the image and caught on screen recordings?

## Checked on main 1dcae03 (lead, 2026-10-30)

- The ADR 80 divisor (`cell_divisor`, `src/graphics/image.rs:289-301`)
  applies only to a picture enlarged by the divisor or more on both
  sides, 3 with cells of 15×39. Neither field-test case qualifies: ten
  768 px images in 2220×2223 px cells are enlarged 2.89 times, and the
  6000×6000 image is shrunk. Both still go out at full cell size, so ADR
  80 has not changed what the field test felt.
- asqr logs only the draw and the write of a frame that sends an image
  (`src/tui/run.rs:411-432`, `debug`, ADR 83); nothing after the write.
- The demo's "asqr scales" mode (`plans/kitty/kitty_scaling.rs` in the
  logbook) is the path from before ADR 80, and its tour leaves out the
  6000×6000 gradient. Before measuring, the demo gets a mode that
  matches asqr's current path (lead decision).
- The todo's 177×69 cells do not fit its 34 MB: 177×69 cells of 15×39
  px are about 38 MB of base64; ADR 80's 33.5 MB was 167×64 cells.

## Questions put to Jakob on 2026-10-30, kept open

Jakob chose "later, keep it open" for all four. Options, the lead's
recommendation first:

- Which cases: both field-test cases on today's code, per image and as
  the total of ten; the ten enlarged images only; the 6000×6000 only.
- Which terminals: Ghostty directly and in tmux; Ghostty plus kitty;
  every Kitty-protocol terminal installed.
- How the visible moment is measured: screen recordings of a stopwatch
  drawn beside the image (by hand, an agent cannot record the screen);
  also a debug line for asqr's next frame; logs and queries only.
- What closes the todo: a table of visible moment against logged times
  per case and terminal, the cause of each gap, and a decision whether
  asqr changes, a fix being its own todo; a threshold Jakob sets and a
  fix that meets it; a written explanation without measuring.

## Re-checked (Jakob, 2026-10-31, in the chat)

Kept parked until Jakob has time for the screen recordings, which only
he can make. Rejected: resuming now in two parts (an agent changes the
demo, then Jakob records about four runs). The questions on the cases,
terminals, measurement and the result that closes the todo come when it
is taken up.

## Revisit when

Jakob has time to record the demo runs in a real terminal.
