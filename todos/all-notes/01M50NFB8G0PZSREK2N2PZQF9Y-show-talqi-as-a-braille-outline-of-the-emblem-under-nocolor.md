---
title: "Show Talqi as a braille outline of the emblem under NO_COLOR"
kind: feature
component: tui
origin: request
tags: [ux]
---
# Show Talqi as a braille outline of the emblem under NO_COLOR

Decided by the user on 2026-10-15. Under `NO_COLOR` the empty queue no
longer shows Talqi's picture, neither through an image protocol nor as
coloured half blocks. Instead it shows a fixed text template: the
outline of the terminal emblem (`assets/emblem/talqi-terminal.svg`)
drawn in braille characters, without any escape sequence. This replaces
decision 12 of the redesign (the half-block mascot keeps its RGB colours
under `NO_COLOR`) and the matching paragraphs of ADR 39, the ADR 15
amendment and the design guide's "Terminal interface" exception.

## What was decided

- The template is the same for the awake and the sleeping state; under
  `NO_COLOR` there is no difference between them.
- Braille over quadrant blocks, half blocks and pure ASCII; the user
  chose it from a sheet of the three.
- It is one text for light and dark terminals: only the outline is ink,
  so it reads as bright lines on dark and dark lines on light.
- Context images in sessions are unchanged: they keep the image
  protocol under `NO_COLOR`, since without it the feature is gone.

## The template

44 columns by 22 rows. Blank cells are U+2800 (the empty braille
pattern), not spaces; drawing should skip them so the background stays
untouched, as transparent half-block pixels do today.

```text
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣤⡶⠿⠛⠛⠿⢶⣦⣀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣰⡿⠋⠀⠀⠀⠀⠀⠀⠈⠻⣷⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣿⠃⠀⠀⠀⠀⠀⠀⠀⠀⠀⠘⣿⡄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣀⣤⡾⠏⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢹⣿⡄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣾⠟⠁⣀⣀⣤⣤⣤⣀⠀⠀⠀⠀⠀⠀⠀⣸⠉⢿⡄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⣸⣯⠶⠛⠉⠉⠀⠀⠈⠙⢷⡄⠀⠀⠀⠀⢠⡟⠀⠘⣿⣄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⠀⠀⢀⣴⡾⠋⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⢻⡄⠀⠀⣰⠟⠀⠀⠀⠀⠙⠿⣦⡀⠀⠀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⠀⢀⣴⠟⠁⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠈⢷⣤⠞⠁⠀⠀⠀⠀⠀⠀⠀⠈⠻⣦⡀⠀⠀⠀⠀⠀⠀
⠀⠀⠀⠀⠀⢠⣾⠋⢀⣤⠶⠖⠲⠶⢦⣄⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⢀⣠⡴⠶⠖⠲⠶⣤⡀⠙⣷⡄⠀⠀⠀⠀⠀
⠀⠀⠀⠀⢠⣿⢃⡴⠋⢀⣀⣀⣀⣀⠀⠈⠳⡄⠀⠀⠀⠀⠀⠀⠀⠀⢠⠟⠁⠀⣀⣀⣀⣀⡀⠙⢦⡘⣿⡀⠀⠀⠀⠀
⠀⠀⠀⠀⣼⡇⣾⣡⡾⠋⣩⣴⣾⡿⠿⣦⡀⢹⠀⠀⠀⠀⠀⠀⠀⠀⠇⢠⣶⠿⢿⣷⣦⣍⠙⢶⡌⣷⢹⣇⠀⠀⠀⠀
⠀⠀⢀⣾⠏⢸⣿⢻⠁⢸⣿⣿⣿⣀⢀⣼⣿⡀⠀⠀⠀⠀⠀⠀⠀⠈⢠⣿⣇⡀⣠⣿⣿⣿⡆⠘⡿⣿⠀⠹⣷⡀⠀⠀
⠀⠀⢸⡏⠀⠈⣿⠸⡄⢸⣿⣿⣿⣿⣿⣿⢻⠃⠀⠀⠀⠀⠀⠀⠀⠀⢸⡽⣿⣿⣿⣿⣿⣿⠇⢰⠃⡿⠀⠀⢹⡇⠀⠀
⠀⠀⢸⣇⠀⠀⠘⢦⡙⢦⣙⠻⠿⠿⢛⡡⠋⠀⢠⣤⡀⠀⠀⢀⣤⠄⠀⠳⣌⡛⠿⠿⠟⣉⠴⣃⠜⠁⠀⠀⣸⡇⠀⠀
⠀⠀⠈⢿⣆⠀⠀⠀⣈⣑⣚⣛⠛⠛⠉⠀⠲⣄⡀⠉⠀⠀⠀⠈⠉⣀⣤⠆⠈⠙⠛⠛⣛⣓⣋⣁⠀⠀⠀⣰⡿⠁⠀⠀
⠀⠀⠀⠀⠙⣷⡶⠛⠉⠉⠉⠉⠛⢷⣄⠀⠀⠀⠉⠛⠛⠒⠒⠛⠋⠉⠀⠀⠀⣠⡾⠛⠉⠉⠉⠉⠛⢶⡾⠋⠀⠀⠀⠀
⠀⣠⣴⠶⠶⡿⠀⢠⡇⠀⠀⢸⡄⠀⢻⡶⠶⠶⠶⠶⠶⠶⠶⠶⠶⠶⠶⠶⢶⡏⠀⢰⡇⠀⠀⢲⡀⠀⢿⠶⠶⣦⡄⠀
⢰⡿⢰⠚⠛⢷⣄⣸⣧⡀⢀⣼⣇⣀⡾⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⠛⢧⣀⣸⣇⠀⢀⣾⣃⣠⡿⠛⠓⡆⣿⡄
⢸⡇⢸⠀⠰⣤⡉⠉⠉⠙⠋⠁⠉⠉⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠉⠉⠈⠛⠋⠉⠉⠉⠀⠀⠀⡇⣿⡇
⢸⡇⢸⠀⢀⣨⡿⠗⢀⣀⣀⡀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⠀⡇⣿⡇
⢸⣧⢸⣀⣈⣁⣀⣀⣉⣉⣉⣁⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⣀⠇⣿⠇
⠀⠻⢶⣬⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣭⣥⡾⠏⠀
```

## How it was made

Generated on 2026-10-15 from a copy of `talqi-terminal.svg` whose
terminal bar is an outlined box with the prompt inside instead of a
dark slab. The copy was rendered at 800 px; the outline mask is every
pixel darker than 0.30 after a percentile stretch inside the figure,
united with the edge of the alpha silhouette, built at 4x supersampling,
fitted in cell units (a cell is about 1:2), and pooled to each cell's
2×4 braille dots at a coverage of 0.30. The scripts and every variant
compared are in the logbook
`~/playground/2026_10_15_asqr-maskottchen/ascii/` (`outline.py`,
`gen.py`, `mksleep.py`, `txt/ohw44-braille.txt`).

## Open points for the implementation

- Size rule: the template is fixed at 44×22; it needs a main area of at
  least 22 rows for it, a blank row and the sentence, and 44 columns.
  Smaller terminals show the sentence alone, as today below the
  picture's threshold.
- Fonts without braille glyphs fall back to another font (Menlo does);
  that was seen in the renders only, not in a real terminal.
- The docs that describe RGB under `NO_COLOR` change with it: ADR 39,
  the ADR 15 amendment, spec section 8, the design guide's exception and
  the `NO_COLOR` screen tests.
- The decision gets an ADR of its own. The user asked that it names
  quadrant blocks (`▘▝▖▗▚▞▛▜▙▟`) as the alternative to switch to if
  braille does not hold up in practice, whether it does not look right
  or causes technical problems such as missing glyphs; the quadrant
  outline of the same
  emblem is `txt/ohw44-quad.txt` in the logbook folder above.
