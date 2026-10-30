# 88. Draw Talqi as his outline behind an overlay

Date: 2026-10-30

## Status

Accepted

Amends [43. Draw Talqi as the emblem in text cells](0043-draw-talqi-as-the-emblem-in-text-cells.md):
Talqi is the outline also behind an overlay

Amends [84. Dim the screen behind an overlay](0084-dim-the-screen-behind-an-overlay.md):
the dimming takes the fill off the submit button and a diff's changed
lines as well as the focus fill

## Context

- `dim_behind` adds `Modifier::DIM` to the cells behind an overlay and
  leaves their background colours as they are.
- Of the 561 visible cells of the quadrant emblem (a glyph or a
  background), 520 have a background colour and 26 more are reversed.
- The submit button and the changed lines of a diff are reversed cells in
  a named colour of the theme.
- asqr knows no RGB value for named ANSI colours and for the terminal's
  default colours, and detects no colour depth.

## Decision

- Behind an overlay Talqi is drawn as his outline, the art asqr already
  draws under `NO_COLOR`. The outline sets no colour. The empty queue is
  told to draw it when the frame has an overlay open; `dim_behind` does
  not look for Talqi and stays a rule over cells.
- `dim_behind` takes REVERSED off the fills asqr draws itself, the way it
  took the focus fill off: the submit button and a diff's changed lines
  show as dim text in their colour, and the focus fill still loses its
  colour too. A cell is one of these fills by its reversed foreground
  colour, one of the named colours of the theme, as for the focus fill.
- Content an entry brings, such as ANSI art, keeps its look: its reversed
  cells keep their reversal. The text cursor does too.
- The cells of a Kitty image keep their style, as before.
- Rejected: a greyscale of every cell's brightness, since asqr has no
  brightness for named colours and the default colours; a greyscale for
  RGB colours only, since asqr detects no colour depth and cannot know
  the theme the other colours are drawn in; blending RGB backgrounds
  toward dark, which changes Talqi only.
