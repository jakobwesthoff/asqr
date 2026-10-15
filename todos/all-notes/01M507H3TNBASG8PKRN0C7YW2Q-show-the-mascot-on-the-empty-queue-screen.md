---
title: "Show the mascot on the empty queue screen"
kind: feature
component: tui
origin: discussion
tags: [ux]
---
# Show the mascot on the empty queue screen

Chosen by the user on 2026-10-15 in the review of the TUI's look. With no
session waiting, the main area holds one centred sentence, "Nothing to
answer. New sessions show up here as soon as they arrive.", and is
otherwise empty. The mascot fills that space.

## Decisions

- 2026-10-15, user: the picture is drawn as a real image through the
  terminal's image protocol, the way context images are. Half blocks in the
  picture's own colours are only the fallback when no image can be shown.
- 2026-10-15, user: on the very first start of asqr on a machine, the
  empty queue shows the base mascot (`assets/mascot-1536.png`). Every other
  time the queue is empty, the sleeping mascot shows
  (`assets/mascot-variants/mascot-sleeping-no-moon.png`).
- 2026-10-15, user: both pictures and their half-block versions are built
  into the executable, so an installed asqr needs no separate files.

## Open questions

- How asqr knows it runs for the first time, for example a marker in the
  data directory, and whether a new queue counts as a first start.
- The size the pictures are embedded at (the PNGs in `assets/` are
  1536 × 1536 and about 2.5 MB each) and from which terminal size the
  picture shows at all; below that only the sentence stays.
- The half-block fallback is drawn in the picture's own RGB colours, an
  exception to the design guide's rule of terminal colours only, which the
  guide should then name for illustrations.
- Moving from the empty screen to the first session must clear the image;
  today `View::must_clear` in `screen.rs` handles overlays closing over an
  image, not this change.
