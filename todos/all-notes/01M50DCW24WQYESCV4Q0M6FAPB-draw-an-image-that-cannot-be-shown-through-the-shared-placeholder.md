---
title: "Draw an image that cannot be shown through the shared placeholder"
kind: refactor
component: tui
origin: review
---
# Draw an image that cannot be shown through the shared placeholder

Found in a refactor review of the TUI's rendering on 2026-10-15. Two
pieces of the context pane exist twice:

- The placeholder for an image that cannot be shown ("image not shown",
  the path, the reason) is built by hand in `Image::draw`
  (`src/tui/render/images.rs` l.80-89 at `a3768f6`) and wrapped by a
  `Paragraph`, while every other entry uses `Placeholder`
  (`src/tui/render/viewer.rs` l.233-285), whose own doc says it is made
  "the way an image placeholder names its file". `ImageViewer::lay_out`
  reports the whole body as its size (viewer.rs l.210-215), so a long
  reason on a short pane is clipped without the position the other
  placeholders show.
- The rule for which paths may be opened (no relative path, the
  metadata must load, only a regular file so a FIFO never blocks) is
  written in `Image::load` (images.rs l.38-51) and in `read_source`
  (`src/tui/render/source_file.rs` l.68-80), with the same reason strings
  and tests of the same names in both files.

## Proposal

`source_file::regular_file(path) -> Result<(), Unreadable>` used by both
loaders; `Image::Unavailable` carries the `Unreadable`, and the image
viewer draws `unreadable.placeholder("image not shown")` like the other
viewers.

## Done when

- One place holds the path rule, one test set covers it.
- The snapshot `a_question_with_an_image_placeholder_at_60_columns` is
  checked: `Placeholder` wraps with asqr's own wrapping, the current code
  with ratatui's; whether the text stays the same has not been run.
- Spec section 7.7 says that a long placeholder scrolls, if it now does.
