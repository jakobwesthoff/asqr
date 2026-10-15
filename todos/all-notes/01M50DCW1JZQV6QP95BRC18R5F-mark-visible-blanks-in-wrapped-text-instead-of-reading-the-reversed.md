---
title: "Mark visible blanks in wrapped text instead of reading the reversed style"
kind: refactor
component: tui
origin: review
---
# Mark visible blanks in wrapped text instead of reading the reversed style

Found in a refactor review of the TUI's rendering on 2026-10-15. The
wrapping code decides from a glyph's style whether a blank is visible:
`wrap_rows` (`src/tui/render/screen.rs` l.1219-1222 at `a3768f6`) keeps a
blank with the `REVERSED` modifier as content and starts the next row
with it at full width, and `mark_cut` (l.750) counts a reversed blank as
text. Reversed blanks are the text cursor (`src/tui/text_field.rs`
l.641, l.650) and the focus fill. So the style of the fill decides how
text wraps: a fill drawn another way, such as a background colour,
would change line breaks, and a head that loses its fill while the
context pane has the focus can leave a trailing blank hanging at exactly
the row width.

## Proposal

`Glyph` carries a flag such as `visible_blank`, set where cursor and fill
glyphs are made; `wrap_rows` and `mark_cut` read the flag instead of the
modifier. The text cursor and the fill keep their looks.

## Done when

- No code outside drawing reads `REVERSED` to decide layout
  (`rg "REVERSED" src/tui` finds only styles that are drawn and tests).
- The existing wrapping and marker tests pass unchanged.
