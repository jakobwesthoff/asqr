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
- 2026-10-15, user: when asqr starts, the empty queue shows the base
  mascot (`assets/mascot-1536.png`). Once sessions have come in and the
  queue is empty again, it shows the sleeping mascot
  (`assets/mascot-variants/mascot-sleeping-no-moon.png`). This holds per run
  of asqr; nothing is stored on disk.
- 2026-10-15, user: both pictures are built into the executable, so an
  installed asqr needs no separate files. They are embedded as 768 px
  lossy WebP (quality 90, full alpha quality, 71 KB and 68 KB) under
  `src/tui/render/mascot/`, because `Cargo.toml` excludes `assets/` from
  the published crate. The half-block fallback is computed from them at
  run time. 768 px because a terminal with cells 15 px wide needs 660 px
  for 44 columns; the first choice was 512 px.
- 2026-10-15, user: the picture is drawn at about the size of the half-block
  fallback, 44 columns by 22 rows, smaller where the space is smaller, not
  stretched to fill the area.
- 2026-10-15, user: the picture shows only when the main area has at least
  12 rows and 30 columns; below that the sentence stays alone.

- 2026-10-15, review: ratatui-image neither enlarges with `Resize::Fit`
  nor centres, so asqr computes the picture's cells from the terminal's
  cell size, centres them, and scales the picture itself with alpha
  premultiplied, since the transparent pixels store black.
- 2026-10-15, review: no clear is needed when the first session replaces
  the picture. ratatui writes over cells an image skipped in the last
  frame, because a cell's diff option is part of its equality.
- The half-block fallback is drawn in the picture's own RGB colours. The
  design guide names that exception for illustrations when it lands.
