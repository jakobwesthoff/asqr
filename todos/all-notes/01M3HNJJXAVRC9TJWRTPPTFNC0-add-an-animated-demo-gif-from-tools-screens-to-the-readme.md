---
title: "Add an animated demo GIF from tools/screens to the README"
kind: docs
component: project
origin: idea
---
# Add an animated demo GIF from tools/screens to the README

The README shows no picture of asqr in use. An animated GIF at its top
shows a person answering a short session. It is drawn by the picture tool
`tools/screens` (ADR 89), one frame per key, so a release refreshes it
with the same `just screens` that refreshes the guide's pictures.

## Decisions

- **Deferred (Jakob, 2026-09-27):** "create a todos file about the vhs
  idea but do not do that now". The idea then was a VHS recording.
- **GIF from `tools/screens` (Jakob, 2026-10-31, in the chat).** ADR 89
  rejects VHS tapes for documentation pictures and keeps one picture
  toolchain. Rejected: a VHS tape as first planned (two toolchains, and
  it reverses ADR 89), a still picture (shows no interaction), keeping it
  open.
- With that option, as described to Jakob: a session written for the
  demo in `examples/sessions/`, a short typed answer and a choice, ending
  on the review tab, as `tools/screens` refuses the submit; the GIF is
  committed under `docs/` and linked from the README, with a size budget
  under 1 MB.

## What it needs

- `tools/screens`: a scene option that keeps every frame
  (`src/play.rs`), a frame delay in the scene format (`src/scene.rs`),
  writing the GIF (`src/lib.rs`), and a GIF encoder in its
  `Cargo.toml` (a dev tool, not published).
- ADR 89 says that `tools/screens` writes GIFs too. It came after
  0.15.0 (lead, 2026-10-31), so while no release contains it, it is
  changed in place; after a release a new ADR amends it.
- The frames are drawn cells, so the GIF shows cell changes, not a real
  terminal's timing.
