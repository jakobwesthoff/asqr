---
title: "Draw ligatures in the documentation pictures as Ghostty shapes them"
kind: chore
component: project
horizon: someday
origin: discussion
tags: [ux]
---
# Draw ligatures in the documentation pictures as Ghostty shapes them

The screens tool (`tools/screens`, the first step of the user guide)
draws asqr's screens into PNG cell by cell, in the look of Jakob's
Ghostty: Victor Mono, Gruvbox Material colours, Ghostty's cell
proportions. Ghostty shapes text with the font features `calt`,
`clig` and `liga` switched on in Jakob's config, so code and diffs
show Victor Mono's ligatures there (for example `->`, `=>`, `!=`).
The pictures do not, so they look a little plainer than the terminal.
Discussed on 2026-10-30: a later polish of the pictures, not needed for
the guide. It concerns the picture tool only; asqr never draws pixels
and does not change.

## Sketch (lead, 2026-10-30)

- Shape each row with `rustybuzz` (a pure-Rust port of HarfBuzz) and
  the features `calt`, `clig` and `liga`: consecutive cells of one
  face (regular, bold, italic, bold italic) form a run, and a glyph from
  a fallback font (DejaVu, Noto) ends a run. Ghostty's own rules for
  where a run ends are read from its source, as the cell metrics were.
- Draw the shaped glyphs by glyph id into their cells. Find out first
  whether Victor Mono's ligatures replace each character by a glyph one
  cell wide or draw one glyph across several cells; both can be drawn.
- Tests: `->`, `=>` and `!=` come out as ligatures; a picture of
  one code line compared with a screenshot of the same line in Ghostty.
- Size: about 150 to 250 lines with tests. `rustybuzz` becomes a
  dependency of the tool only, not of asqr.

## Revisit when

`tools/screens` is on main and the guide's pictures exist, and the
pictures should look closer to the terminal.

## Relations

- Relates to: [Write a user guide with screenshots, every key and the agent setup](01M50JVCC635YS0REQGD31MBBQ-write-a-user-guide-with-screenshots-every-key-and-the-agent-setup.md)
