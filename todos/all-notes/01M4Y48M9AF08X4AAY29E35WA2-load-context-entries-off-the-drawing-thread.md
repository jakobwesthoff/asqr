---
title: "Load context entries off the drawing thread"
kind: bug
component: tui
impact: low
origin: review
tags: [ux]
---
# Load context entries off the drawing thread

Found in the adversarial review of the 0.11.0 work on 2026-10-14. The
viewers of code, Markdown, diff and image entries read their files on
the thread that draws the screen, so a slow or odd file freezes the
whole interface until the read ends.

## Problem

`read_source` in `src/tui/render/source_file.rs` and `Image::load` in
`src/tui/render/images.rs` run from the viewers' `load` while the
person waits for the frame. Commit `30e4d3d` limits the damage: only
regular files are read, checked with a stat before the open, and text
stops after 8 MiB (ADR 0033, "Only regular files, and at most 8 MiB").
Two gaps remain, read in the code (2026-10-14):

- A path swapped for a FIFO between the stat and the open still blocks
  until a writer appears.
- Reading and highlighting up to 8 MiB, or decoding a large image,
  stalls the screen for as long as it takes, on a slow or network file
  system longer still.

## Impact

No key takes effect while a read hangs, since keys are handled between
frames: the person cannot type, scroll or quit.

## Proposal

Load each entry on a worker thread and show a dim "loading…" in the
pane until the result arrives; the run loop redraws when it does.
Opening with `O_NONBLOCK` (then clearing it) closes the FIFO gap even
before the move.

## Open questions

- Whether a load that is no longer shown (the person moved on) is
  dropped or cached.
- How the result reaches the run loop: through the existing event
  channel in `src/terminal.rs` or a channel of its own.
