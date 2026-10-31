---
title: "Draw an image partly scrolled off in a pane"
kind: improvement
component: tui
horizon: someday
origin: discussion
tags: [ux]
---
# Draw an image partly scrolled off in a pane

asqr draws an image only while all of it fits its area. Building the
image caption on 2026-10-28 showed that ratatui-image 12.0.0-rc.3 can
draw an image partly scrolled off, which a pane that scrolls an image
together with text would need. The user kept the caption layout "Fit"
(image and caption always shown together, nothing scrolls) instead, so
nothing needs this now.

## What ratatui-image offers

`ratatui_image::sliced::SlicedImage::new(&SlicedProtocol, SignedPosition)`
draws an image as if it started `y` rows above the area. `SlicedProtocol`
is a public enum with the variants `Kitty`, `Halfblocks`, `Sliced` (one
`Iterm2` protocol per row) and `Sixel`. `Image::allow_clipping(true)`
clips only at the right and bottom, so it does not cut the top.

## Per protocol, as investigated on 2026-10-28 (read-only, asqr at 313bd88)

- Half blocks: asqr builds its `Halfblocks` itself
  (`src/graphics/image.rs`, `encode`) and can wrap it in
  `SlicedProtocol::Halfblocks`. Pure cells, testable in buffers; low
  risk.
- Kitty: asqr's own `Kitty` (own id, own compression) goes into
  `SlicedProtocol::Kitty`, and scrolling then sends only cells, no image
  data. `PlacedImage` in `src/graphics/placeholders.rs` assumes the
  image's row 0 at the area's top: `mend` writes `DIACRITICS[y - area.top()]`
  and `read` uses the area the same way, so both need the skipped rows
  as an offset, or mended rows name the wrong image rows. Moderate risk,
  testable through placeholder cells.
- iTerm2: only by cutting the resized picture into strips of one cell
  row and encoding each as an `Iterm2`, since the crate builds its
  `Sliced` variant only from a `Picker`. Seams from rounding, more bytes
  per frame, and only a real terminal shows the result; medium-high
  risk.
- Sixel: `SlicedSixel` is built only through a `Picker`, whose every
  constructor runs `tmux set -p allow-passthrough on`, which asqr avoids
  (ADR 44). Reimplementing the band slicing is about 150 lines and cuts
  land on 6-pixel bands. The fallback is blank rows once the top of the
  image has scrolled off.

Estimate for half blocks and Kitty: about 200 to 300 lines with tests.

## Re-checked (Jakob, 2026-10-31, in the chat)

Kept for later: no screen scrolls an image with its text, since captions
use the Fit layout (ADR 69). Rejected for now: partial drawing in half
blocks and Kitty only, partial drawing in all four protocols, and closing
the todo.

## Revisit when

A pane needs to scroll an image together with other content, or the
caption layout is reconsidered. The move to a final ratatui-image
release (todo 01M6217WNNH2P5GF1545D08C2Z) may change the `sliced` API.
