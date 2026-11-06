---
title: "Draw Talqi's emblem for the cell shape of the terminal"
kind: bug
component: tui
impact: low
origin: discussion
tags: [ux]
---
# Draw Talqi's emblem for the cell shape of the terminal

The emblem of the empty queue is drawn for cells half as wide as they
are high, and in many fonts a cell is narrower than that, so Talqi
shows as an upright oval instead of round. Seen in the antiq project on
2026-11-06 with the same kind of chafa rendering at 44 × 22 cells: in
Ghostty with Victor Mono 14 pt the emblem looked squeezed; the cell
there is 15 × 39 px (tmux `client_cell_width`/`client_cell_height`),
a ratio of 0.385, so the 44 × 22 block is 660 × 858 px, about 23 %
too narrow. Not yet looked at in asqr itself, but asqr's emblem is made
the same way.

## Facts

- asqr's emblem is a fixed block of 44 columns by 22 rows
  (`src/tui/render/mascot/template.rs`, `EMBLEM_COLUMNS`,
  `EMBLEM_ROWS`; ADR 43), made by `scripts/emblem-cells.py` with chafa
  `--size 44x22` and chafa's default font ratio of 1/2.
- Cell width over line height from the font files (`hhea` advance of
  M over ascent − descent + line gap), measured 2026-11-06: Victor Mono
  0.387, Monaco 0.450, JetBrains Mono 0.455, Fira Code 0.500, Menlo,
  SF Mono and DejaVu Sans Mono 0.517 to 0.518, Courier New, Andale Mono
  and PT Mono 0.530 to 0.536. A terminal's line spacing setting shifts
  these further. One fixed rendering is therefore round in only some of
  them.
- An error of 23 % was plain to see; below about 5 % it hardly shows.
  Four renderings at ratios 0.39, 0.43, 0.47 and 0.52 keep every font
  in that range within about 5 % of round; at 22 rows they are 56, 51,
  47 and 42 columns wide. Three (0.40, 0.45, 0.51) keep it within about
  7 %; a fifth gains little, since the width moves in whole columns.
- asqr already knows the cell size in pixels: `Graphics::cell`
  (`src/graphics/mod.rs`) from ratatui-image's picker, 10 × 20 when the
  terminal does not answer.

## Suggested fix

Render the emblem (colour and outline, sextant and quadrant) for each of
the four ratios, keeping 22 rows, and pick the rendering nearest to
`cell.width / cell.height` at start; without an answer keep today's
1/2 rendering. The empty-queue layout then needs the width of the chosen
rendering instead of the fixed 44 columns. antiq does the same with
`scripts/emblem-ansi.sh` (`just emblem-ansi`, chafa `--font-ratio`
per rendering).

## Relations

- Relates to: [Check image handling and the emblem in real terminals](01M51VD1CY4XM1PSF63S0JGP45-check-image-handling-and-the-emblem-in-real-terminals.md)
