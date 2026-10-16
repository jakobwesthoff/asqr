---
title: "Overlays blank the whole rows of a Kitty image they cross"
kind: bug
component: tui
impact: high
horizon: next
origin: implementation
tags: [ux, field-test]
---
# Overlays blank the whole rows of a Kitty image they cross

Seen by Jakob on 2026-10-16 in Ghostty under tmux, release build of
`9b22439`: while the help (`?`), the intro (`i`) or the session list
(`L`) is open over a context image, the image is gone across its full
width in every row the overlay covers. A second bug of the same kind
came up while preparing the fix: an image first drawn under an overlay
is never sent.

## Problem

Under the Kitty protocol ratatui-image (12.0.0-rc.2,
`src/protocol/kitty.rs`) draws an image as cells of `U+10EEEE`:

- Only the first cell of each image row carries the row, column and
  id diacritics; every other cell inherits its position from the
  placeholder to its left (the `if x == 0` branch, pinned by the test
  `only_the_first_cell_of_a_row_states_a_position`). An overlay writes
  its own cells over some of them, and a placeholder whose left
  neighbour is no longer one has nothing to inherit: the terminal draws
  nothing there.
- The image data is prefixed to the first image cell, once per encoding
  (lines 193-199, an `AtomicBool`). When an encoding is first drawn
  while an overlay covers that cell (after a resize, on regaining the
  focus under tmux `on`, or when a replaced session file changes the
  image while an overlay is open), the overlay overwrites the data and
  it never reaches the terminal: the image stays blank after the
  overlay closes, until the next encoding. ratatui-image cannot fix
  this, as it never sees what is drawn after the image.

## Decisions (Jakob, 2026-10-16, asqr session 01M51YY3B8K8ZX5PA5F369ZJB1)

- Fix it in asqr; send nothing upstream.
- Both bugs are fixed in this todo.

## Design (decided by the lead)

- After the overlays are drawn, mend the frame: every placeholder cell
  of an image whose left neighbour is not a placeholder of the same
  image gets its row, column and the id's high byte as diacritics
  (always valid by the spec's rules; asqr's ids are 32-bit random, so
  the high byte is needed). Measured: 15 cells and +90 bytes with the
  help open; stamping every cell instead would cost 6 to 22 KB per
  redraw.
- Move an image's transmission to its first cell still drawn, or to
  buffer cell (0,0) when the image is fully covered, keeping its
  `ForcedWidth`.
- The frame's placements come back through `draw_pane` into `View`,
  not into `KittyBook`, whose bookkeeping the architecture review of
  2026-10-16 wants simpler.
- The diacritic table is copied from the Kitty spec's
  `rowcolumn-diacritics.txt` (297 marks), with a test that compares it
  with ratatui-image's output.
- Tests need wide screens: the help is 70 columns wide and centred, so a
  gap beside it shows from about 200 columns.

A tested prototype on `f5f26a3` (mending pass, transmission move and the
two screen tests `an_overlay_over_a_kitty_image_leaves_the_cells_beside_it_in_place`
and `an_image_first_drawn_under_an_overlay_is_still_sent`, both red
without the fix) is in
`~/playground/2026_10_15_asqr-maskottchen/vorbereitung/kitty-overlay-prototype.diff`;
it still keeps the placements in `KittyBook` and reads the diacritic
table off ratatui-image.
