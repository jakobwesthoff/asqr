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
seen by Jakob on 2026-10-16. Context entries are read, highlighted,
rendered, decoded, scaled and encoded on the thread that draws the
screen, so a large entry freezes the whole interface until the work
ends: no key takes effect, and nothing says why.

## Problem

Measured on 2026-10-16 with a release build of `f5f26a3` (the
preparation round; measurements and scripts in Jakob's logbook,
`~/playground/2026_10_15_asqr-maskottchen/vorbereitung/loading/`):

- Building a viewer reads its file and highlights its text in the
  constructor (ADR 46). Markdown rendering
  (`src/tui/render/markdown_viewer.rs:89-100`) and the scaling and
  encoding of images (`viewer.rs:283` into `src/graphics/image.rs:80-103`)
  run in `lay_out`, also on the drawing thread, and again on every width
  change.
- The 8.5 MiB test log (asqr shows its first 8 MiB, 113,518 lines):
  `patine::highlight` with the `log` grammar takes 23.0 s, linear at
  0.19 ms per line. Reading takes 5.5 ms, plain lines 110 ms,
  `CodeViewer::new` 31.7 s in all. A Rust file of 941 lines highlights in
  50 to 70 ms.
- Images cost 80 to 380 ms of CPU from read to encoded frame, a
  6000×6000 PNG 790 to 890 ms. The transfer of an encoded image through
  tmux costs far more, but that runs inside `terminal.draw` and is the
  matter of its own todo (see Relations).
- A path swapped for a FIFO between the stat and the open blocks until
  a writer appears (read in the code, 2026-10-14).

## Decisions (Jakob, 2026-10-16, asqr session 01M51Z9860935MZKGH148W7PSB)

- A worker builds the viewer and lays it out, in one form for every
  kind of entry.
- While an entry loads, the pane shows a dim `loading…`, centred in the
  pane.
- asqr waits up to 100 ms for the result before it shows the
  placeholder, so quick entries never flash it.
- A load whose entry is no longer shown runs to its end and is kept.
- Loading ahead: the entry shown always goes first; then the other
  options of the current question, then the next question.
- Highlighting: text above 1 MiB stays plain. Below that, an entry shows
  once it is highlighted; there is no plain first pass that colours in
  later.
- ADR 46 is rewritten to the new design (no new ADR that amends it).

## Decided by the lead (delegated)

- Results come back on a channel that `Viewers` owns; a payload-free
  `Event::Loaded` through the existing sender (`src/terminal.rs:57`)
  only wakes the run loop, since `Event` derives `Debug`, `Clone` and
  `Eq` (`run.rs:25`) and the loop blocks in `events.recv()`
  (`run.rs:129`). Viewer trait objects become `Send`; `Images` is
  `Clone` and its book sits behind `Arc<Mutex>`.
- The entry shown gets a thread at once; loading ahead runs on one
  background thread in priority order. patine cannot be cancelled, so a
  long highlight only holds that thread.
- Open files with `O_NONBLOCK` and clear it after the open, or a FIFO
  holds a worker for ever.
- Mind memory when loading ahead: an image keeps its decoded source for
  re-encoding (`image.rs:33-37`), 144 MB for the 6000×6000 PNG.
- `forget_images` on regaining the focus (`viewer.rs:164`) re-reads and
  re-decodes every image; with the worker this becomes a reload behind
  the placeholder.
