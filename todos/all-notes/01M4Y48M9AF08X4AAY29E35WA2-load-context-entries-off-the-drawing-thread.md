---
title: "Load context entries off the drawing thread"
kind: bug
component: tui
impact: high
horizon: next
origin: review
tags: [ux, field-test]
---
# Load context entries off the drawing thread

Found in the adversarial review of the 0.11.0 work on 2026-10-14 and
seen by Jakob on 2026-10-16. The viewers of code, Markdown, diff and
image entries read, highlight or decode their content on the thread
that draws the screen, so a large or slow file freezes the whole
interface until the work ends.

## Problem

Since ADR 46 a viewer loads in its constructor: building it reads the
file (`read_source` in `src/tui/render/source_file.rs`), highlights the
whole text with patine (`src/tui/render/code.rs`), computes the diff or
decodes the image (`Images::load` in `src/graphics/mod.rs`). The viewer
is built the first time its entry shows, on the drawing thread. Commit
`30e4d3d` limits the damage: only regular files are read, checked with
a stat before the open, and text stops after 8 MiB (ADR 0033). Two gaps
remain:

- Reading, highlighting or decoding a large entry stalls the screen for
  as long as it takes. Jakob, 2026-10-16, release build of `9b22439`
  under tmux: moving to a question whose context is an
  8.5 MiB log (120,000 lines) took "wahnsinnig lange" and blocked the
  whole UI. Which step costs the time (read, highlighting, building the
  lines) is not measured yet.
- Images stall the same way. Same build and terminal (Ghostty, tmux),
  a `multi` question whose ten options each carry a 768 px image in
  another format: the first move onto each option took "ewig", moving
  back was quick. Yet a 6000×6000 PNG (14 MB) on a question of its own
  showed almost at once. Why the small images are slower is not
  understood; measure decoding, scaling, encoding and the transfer
  through tmux separately before choosing a fix. Jakob asks to handle
  images and text with one form of background loading.
- A path swapped for a FIFO between the stat and the open still blocks
  until a writer appears (read in the code, 2026-10-14).

## Impact

No key takes effect while a load runs, since keys are handled between
frames: the person cannot type, scroll, move on or quit, and the screen
does not say why.

## Proposal

Build each viewer on a worker thread and show a dim "loading…" in the
pane until it arrives; the run loop redraws when it does. ADR 46 gave
up the separate load step and notes that it can come back when loading
ahead of time is needed; this is that case, so the change amends ADR
46. Opening with `O_NONBLOCK` (then clearing it) closes the FIFO gap.

## Open questions

- Whether a load that is no longer shown (the person moved on) is
  dropped or cached, and whether entries of the next question load
  ahead.
- How the result reaches the run loop: through the existing event
  channel in `src/terminal.rs` or a channel of its own.
- Whether highlighting only what the pane shows would remove most of
  the wait on its own; this depends on the measurement above.
