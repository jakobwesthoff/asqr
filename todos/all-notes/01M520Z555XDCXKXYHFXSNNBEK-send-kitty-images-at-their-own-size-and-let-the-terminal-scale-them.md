---
title: "Send Kitty images at their own size and let the terminal scale them"
kind: improvement
component: tui
impact: high
origin: implementation
tags: [ux, field-test]
---
# Send Kitty images at their own size and let the terminal scale them

Found while preparing the loading todo on 2026-10-16. Under the Kitty
protocol asqr scales every context image to the pixels of its pane
before it encodes it, also upwards, and the encoded picture goes
through tmux far more slowly than anything else asqr does.

## Problem

Measured with a release build of `f5f26a3` (scripts and numbers in
`~/playground/2026_10_15_asqr-maskottchen/vorbereitung/loading/`):

- In Jakob's terminal (249×68 cells, cells of 15×39 px) the context pane
  is 148 columns wide, and every square image goes out at 2220×2223 px.
  A 768 px image is enlarged 2.9 times per side. One transmission is
  26.5 MB raw or 7.3 to 8.9 MB with zlib.
- Through a tmux server with a client attached, one 8.5 MB frame takes
  12.4 s, raw 39.4 s, the same image at its own size (1.2 MB) 0.78 s.
  Without tmux the frame takes 0.02 s.
- The transfer runs inside `terminal.draw`, so loading on a worker does
  not hide it.

Why a 6000×6000 PNG felt quick to Jakob while ten 768 px option images
felt slow is not explained: transfer and CPU cost the same or more for
the large one.

## Proposal

Under Kitty, scale an image only down, to at most the pane's pixels,
and let the terminal enlarge it to its cells. asqr encodes Kitty images
itself (`src/graphics/image.rs:144-168`); ratatui-image's
`transmit_base64` sends the picture's pixels as `s`/`v` and the cells
as `c`/`r` (ratatui-image 12.0.0-rc.2 `kitty.rs:457`), so it does not
force the pre-scaling, which comes from asqr's own `resize.resize`
(`image.rs:101`). Pad the source to the cells' aspect ratio so the
terminal scales it evenly. The triangle filter (commit `f853114`) keeps
its job wherever asqr still shrinks. Sixel and iTerm2 (whose encoding
sends pixel sizes, `iterm2.rs:46`) stay pre-scaled; half blocks are not
affected. Not yet tried in Ghostty.

Before that, log per frame the bytes written and the duration of
`terminal.draw`, and `kitty_compression` in the "graphics protocol
detected" line (`src/terminal.rs:126-131` leaves it out), so a field
test can settle the open question above.

## Decisions (Jakob, 2026-10-16)

Jakob agreed to the proposal (asqr session 01M51Z9860935MZKGH148W7PSB)
and asked to build it last, after the other field-test todos, in a
separate step he tries out right after; the lead reminds him then.

## Relations

- Relates to: [Load context entries off the drawing thread](01M4Y48M9AF08X4AAY29E35WA2-load-context-entries-off-the-drawing-thread.md)
