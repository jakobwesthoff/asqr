# 43. Draw Talqi as the emblem in text cells

Date: 2026-10-15

## Status

Accepted

Amended by [81. Name the chosen queue](0081-name-the-chosen-queue.md):
the sentence under Talqi names a chosen queue

## Context

With no session waiting, the main area holds one centred sentence,
"Nothing to answer. New sessions show up here as soon as they arrive.",
and is otherwise empty. That space shows asqr's mascot, the chameleon
Talqi (pronounced like "talkie"), whom the design guide names, as the
terminal emblem (`assets/emblem/talqi-terminal.svg`) drawn in text
cells, with colours and without.

What the choice of glyphs rests on:

- Sextants (U+1FB00..1FB3B) are drawn by these terminals themselves,
  without a font: kitty from 0.19.0, Ghostty from 1.0.0, WezTerm from
  20210814-124438-54e29167 (`custom_block_glyphs`, on by default),
  Alacritty from 0.13.2, foot from 1.7.0, VTE from 0.60.0, Konsole from
  22.04.0, Rio from 0.4.6, iTerm2 from 3.6.7 (first in 3.6.7beta2) and
  Contour. Terminal.app, xterm, desktop Warp and Windows Terminal's
  stable release depend on the font, and no font that comes with macOS
  has sextants, so a stock Mac shows boxes of a missing glyph there.
- No escape sequence reports whether a terminal can show a glyph.
- The terminals name themselves in the environment and in their answer
  to XTVERSION: `TERM_PROGRAM` with `TERM_PROGRAM_VERSION` (Ghostty
  `ghostty`, WezTerm `WezTerm`, iTerm2 `iTerm.app`, Rio `rio`),
  `LC_TERMINAL=iTerm2` with `LC_TERMINAL_VERSION`, which ssh passes on,
  `TERM=xterm-kitty` or `KITTY_WINDOW_ID`, `TERM=xterm-ghostty`,
  `TERM` starting with `foot`, `VTE_VERSION` (6000 for 0.60),
  `KONSOLE_VERSION` (220400 for 22.04), `TERM=alacritty` or
  `ALACRITTY_WINDOW_ID`, and `TERMINAL_NAME=contour`. kitty and
  Alacritty name no version there. XTVERSION answers start with
  `kitty(`, `foot(`, `WezTerm `, `ghostty `, `iTerm2 `, `VTE(`,
  `Konsole `, `contour ` and `Rio `; Alacritty does not answer it.
- Inside tmux, the environment holds what the terminal tmux was started
  from set. tmux keeps the XTVERSION answer of each client's terminal
  in `#{client_termtype}`.
- Sextants and quadrants (U+2596..259F) are of East Asian width N, as
  is `▐` (U+2590). `▀`, `▄`, `█` and `▌` are Ambiguous: unicode-width
  0.2.2 counts them as 2 columns in CJK mode.

## Decision

What the empty queue shows:

- The standard terminal emblem, the same for the whole run.
- A fixed block of 44 columns by 22 rows. With colours, each cell is a
  glyph with a 24-bit foreground and background. Under `NO_COLOR`, it is
  the emblem's outline without any colour.
- Terminals asqr recognises as drawing sextants themselves get the
  emblem in sextants, every other terminal in quadrants. There is no
  setting to override the choice.
- The glyph sets are chafa's: `sextant`, sextants with `█▌▐`, and
  `quad`, quadrants with `▀▄█▌▐`. asqr assumes narrow width for East
  Asian Ambiguous characters, as it does for `─│╭━↑←…·▀` elsewhere on
  its screens.

The templates:

- Four templates are built into asqr under `src/tui/render/mascot/`:
  colour and outline, each in sextants and in quadrants.
- `scripts/emblem-cells.py` makes them. It is Python with its
  dependencies pinned in PEP 723 metadata and runs with `uv run`. It
  needs `rsvg-convert` and chafa 1.18.2, and refuses another chafa
  version.
- The colour templates come from `assets/emblem/talqi-terminal.svg`
  without the emblem's outer ink rim. The
  SVG's first path is an ink silhouette under all others, and its edge
  is that rim. The script renders the emblem at 800×800 pixels with
  `rsvg-convert` twice, with and without that path. Where the render
  without it has a hole closed all round (the band between chin and
  panel) or a gap that a closing of radius 9 pixels fills (the line
  between crest and head), it puts the silhouette's ink back under it,
  two pixels wider so the anti-aliased edges cover ink. Eyes, mouth,
  nostrils, paw outlines and the panel are paths of their own and stay.
- The script scales that picture with Lanczos to chafa's own grid of
  8×8 pixels per cell, 352×176 pixels, and cuts its alpha at 128, so
  chafa neither scales it nor blends a half transparent edge pixel with
  `--bg`:

      chafa --format symbols --symbols {sextant|quad} --size 44x22 \
          --work 9 --dither none --polite=on --colors full --stretch \
          --bg '#09171a' <picture>.png

  `--polite=on` leaves out the sequences that hide and show the cursor.
  chafa's glyphs at the figure's edge still change with `--bg`; the
  templates are made with the ink colour, `#09171a`.
- A few cells keep a speck of ink where the figure's edge meets an
  inner line: where the line between crest and head reaches the edge,
  and where the left paw's own outline reaches the edge beside the
  body.
- The outline templates come from `assets/emblem/talqi-terminal-hollow.svg`,
  the emblem with its terminal bar drawn as an outlined box with the
  prompt inside. The script renders it at 800×800 pixels and makes a
  mask of it: cropped to the pixels with alpha above 8, fitted with
  Lanczos into 20×40 pixels per cell, its luminance stretched between
  the 2nd and the 98th percentile inside the figure, every pixel below
  0.30 counted as ink together with the Sobel edge of the alpha
  silhouette, blurred by 1.2 pixels. chafa draws that mask, white on
  black:

      chafa --format symbols --symbols {sextant|quad} --size 44x22 \
          --work 9 --dither none --polite=on --colors none \
          --fg white --bg black <mask>.png

- The script checks that every template has 22 lines and that the
  outlines hold no escape character.

Reading and drawing:

- The colour templates hold SGR `0`, `7` (reverse, followed by a
  foreground alone), `38;2;r;g;b`, `48;2;r;g;b` and both colours in
  one sequence. chafa writes a colour only where it changes, so it
  carries over to the cells that follow, and ends every line with a
  reset. asqr reads `0`, `7`, `38;2`, `48;2`, `39` and `49`, alone or
  combined, and nothing else; a test reads every template. Each
  template is read once, when it is first drawn.
- A space whose background is the terminal's own and that is not
  reversed is transparent: asqr leaves that cell as it is, so the
  terminal's background shows there on light and dark themes. Every
  other cell gets its glyph, its foreground and its background where it
  has them, and the reverse attribute where it is reversed. The cells of
  an outline get their glyph and no style.

Size and place:

- Talqi shows when the main area has room for the emblem's 22 rows, a
  blank row and the 3 rows kept for the sentence, 26 rows, and for its
  44 columns, with colours and under `NO_COLOR`, except while the dialog
  of an invalid session shows (ADR 49). Otherwise the sentence stands
  alone. The main area has 6 rows less than the terminal, so
  Talqi needs a terminal of at least 32 rows.
- Emblem, blank row and sentence form one group, centred down the main
  area, with the emblem centred across.

Choosing the glyphs, once at start:

- Without tmux (`TMUX` unset or empty), the environment decides. A
  `TERM` starting with `screen` or a non-empty `ZELLIJ` gives quadrants,
  as asqr cannot learn which terminal is around them. A non-empty
  `TERM_PROGRAM` decides next, also one not on the list, which gives
  quadrants; it comes before `KITTY_WINDOW_ID` and `ALACRITTY_WINDOW_ID`,
  which a terminal started from kitty or Alacritty keeps. Then come
  `LC_TERMINAL`, `VTE_VERSION`, `KONSOLE_VERSION` and the rest of the
  variables above.
- Inside tmux, the environment does not count. The one
  `tmux display-message -p` asqr runs at start (ADR 44) asks for
  `#{client_termtype}` among its fields, separated by tabs since the
  termtype holds blanks, and its answer serves both the graphics query
  (spec section 8.9) and the glyphs. The termtype decides by its prefix
  and version.
  An empty or unknown termtype, or no answer from tmux, gives quadrants.
- Version floors are the first versions above. A version is read as the
  numbers at its start, separated by dots; a pre-release such as
  `3.6.7beta2` counts as its release, and a version that cannot be read
  gives quadrants. kitty, Alacritty, Ghostty, foot and Contour need no
  version.
- tmux with several clients answers for its current client. A tmux
  inside tmux gets quadrants, as the inner tmux's client is a tmux.
  Over ssh into a tmux from Ghostty, the remote tmux's client is
  Ghostty, which gets sextants.

## Consequences

- Talqi does not show on terminals of fewer than 32 rows, 80×24
  included, with colours or without.
- Talqi sends no image, and asqr uses no image protocol and no
  ratatui-image for Talqi.
- Under `NO_COLOR`, no SGR with a colour other than the terminal's
  defaults reaches the terminal while the empty queue shows. A test in a
  pseudo-terminal checks the output for 38, 48 and the palette codes.
- A terminal not on the list gets quadrants even when its font has
  sextants.
- Inside tmux, a terminal without RGB support in tmux's view gets the
  24-bit colours mapped to 256 colours by tmux.
- asqr does not check whether the terminal shows 24-bit colour, as it
  does not for the half blocks of context images.
