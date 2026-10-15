# 41. Show Talqi as a braille outline under NO_COLOR

Date: 2026-10-15

## Status

Accepted

Supersedes Talqi under `NO_COLOR` of [39. Show the mascot on the empty queue](0039-show-the-mascot-on-the-empty-queue.md)

Supersedes the mascot's colours under `NO_COLOR` of [15. Configure through flags and environment and log into the cache directory](0015-configure-through-flags-and-environment-and-log-into-the-cache-directory.md)

## Context

ADR 39 shows Talqi on the empty queue through the terminal's image
protocol, or in half blocks of the picture's RGB colours where the
terminal has none. Under `NO_COLOR`, asqr resets every named and indexed
colour of the drawn screen and keeps RGB colours (ADR 15, amendment of
2026-10-15). The user had decided that Talqi's half blocks keep their
RGB colours under `NO_COLOR`, as images do, and both ADRs record that.

On 2026-10-15 the user reversed that decision for Talqi: under
`NO_COLOR` the empty queue shows Talqi as a fixed text without any
escape sequence, neither through an image protocol nor as coloured half
blocks. The images of context entries keep the image protocol under
`NO_COLOR`, since without it the feature is gone.

## Decision

The outline:

- Under `NO_COLOR`, Talqi is the outline of the terminal emblem
  (`assets/emblem/talqi-terminal.svg`) drawn in braille characters, a
  fixed text of 44 columns by 22 rows. It is built into asqr as
  `src/tui/render/mascot/talqi-outline.txt`.
- The text is the same for Talqi awake and asleep. Under `NO_COLOR`
  there is no difference between the two.
- It is one text for light and dark terminals. Only the outline is ink,
  so it reads as bright lines on a dark background and dark lines on a
  light one.
- A cell without ink holds U+2800, the empty braille pattern. asqr
  skips those cells, so the background stays as it is. A cell with ink
  gets its character and no colour and no modifier.
- Talqi's pictures are neither decoded nor sent under `NO_COLOR`, with
  an image protocol or without one.

Size and place:

- The outline shows when the main area has room for its 22 rows, a
  blank row and the 3 rows kept for the sentence, 26 rows in all, and
  for its 44 columns. Otherwise the sentence stands alone.
- The outline is centred across, and outline, blank row and sentence
  form one group centred down the main area, as the picture does in
  ADR 39.

Braille, and what replaces it:

- The user chose braille from a sheet of three candidates, each the
  same outline at 44×22 on a dark and a light background: quadrant
  blocks, half blocks and braille.
- If braille does not hold up in practice, whether it does not look
  right or causes technical problems such as a font without braille
  glyphs, asqr switches to quadrant blocks (`▘▝▖▗▚▞▛▜▙▟`), as the user
  asked. The quadrant outline of the same emblem is
  `txt/ohw44-quad.txt` in the logbook named below.
- In the renders made for the comparison, a font without braille
  glyphs (Menlo) fell back to another font for them. This was not
  checked in a real terminal.

How the outline was made:

- It was generated on 2026-10-15 from a copy of
  `talqi-terminal.svg` whose terminal bar is an outlined box with the
  prompt inside instead of a dark slab.
- The copy was rendered at 800 pixels. The outline mask is every pixel
  darker than 0.30 after a percentile stretch inside the figure, united
  with the edge of the alpha silhouette, built at 4× supersampling and
  fitted in cell units, a cell being about 1:2.
- The mask was pooled to each cell's 2×4 braille dots at a coverage of
  0.30.
- The scripts (`outline.py`, `gen.py`, `mksleep.py`) and every variant
  compared are in the logbook
  `~/playground/2026_10_15_asqr-maskottchen/ascii/`. The built-in text
  is byte for byte its `txt/ohw44-braille.txt`.

## Consequences

- The main area of the empty queue has 6 rows less than the terminal
  (ADR 39), so under `NO_COLOR` Talqi shows on terminals of at least 32
  rows. With colours, Talqi shows from 18 rows on; between 18 and 31
  rows, `NO_COLOR` shows the sentence alone.
- The outline does not shrink: below its size there is no Talqi under
  `NO_COLOR`.
- While the outline shows, asqr writes no colour sequence (SGR 38 or
  48). A test in a pseudo-terminal checks that.
- asqr does not generate the outline. The text file is its source, and
  the logbook holds the scripts that made it.
