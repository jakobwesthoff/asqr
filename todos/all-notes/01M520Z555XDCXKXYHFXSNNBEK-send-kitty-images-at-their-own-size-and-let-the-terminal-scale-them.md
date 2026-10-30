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
  is 148 columns wide, and every square image goes out at 2220×2223 px,
  19.7 MB of RGBA. A 768 px image is enlarged 2.9 times per side. One
  transmission is 26.5 MB raw as measured, or 7.3 to 8.9 MB with zlib.
- asqr never compresses: `query_graphics` (`src/terminal.rs`) keeps
  `QueryStdioOptions::default()`, whose `kitty_compression` is `false`
  in ratatui-image 12.0.0-rc.3 (`picker/cap_parser.rs` l.138), so the
  `o=z` probe is never sent and `Graphics::kitty_compression` is always
  `false`. Every frame asqr sent was raw.
- Through a tmux server with a client attached, the raw 26.5 MB frame
  takes 39.4 s. The other measured frames are zlib, which asqr does not
  send: 8.5 MB in 12.4 s through tmux and 0.02 s without it, and the
  768 px image at its own size, 1.2 MB, in 0.78 s through tmux.
- Raw at its own size, the 768 px image is 2.36 MB of RGBA, about
  3.1 MB transmitted (base64). The measured rates disagree (26.5 MB in
  39.4 s and 8.5 MB in 12.4 s against 1.2 MB in 0.78 s), so through tmux
  it would take between about 2 s and 4.7 s; not measured.
- The transfer runs inside `terminal.draw`, so loading on a worker does
  not hide it.

Why a 6000×6000 PNG felt quick to Jakob while ten 768 px option images
felt slow is not explained: both went out raw at 2220×2223 px, so
transfer and CPU cost the same or more for the large one.

## Proposal

Under Kitty, scale an image only down, to at most the pane's pixels,
and let the terminal enlarge it to its cells. asqr encodes Kitty images
itself (`src/graphics/image.rs:144-168`); ratatui-image's
`transmit_base64` sends the picture's pixels as `s`/`v` and the cells
as `c`/`r` (ratatui-image 12.0.0-rc.3 `kitty.rs:457`), so it does not
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

## Decisions (Jakob, 2026-10-30, asqr session 01M6628SFCCK00ENY6YV0XQZR1)

- Build it now, no longer after the field test.
- First a simple demo Jakob can start quickly, possibly outside asqr,
  that shows how the two ways behave and look, with a way to switch
  between them.
