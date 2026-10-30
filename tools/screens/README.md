# screens

A development tool of the asqr repository, not published: the pictures
of asqr's documentation are drawn with asqr's own code
([ADR 89](../../docs/adr/0089-draw-the-documentation-pictures-with-asqrs-own-rendering.md)).
This crate holds the fonts the pictures are drawn with, with their
licences, and turns a drawn ratatui buffer into a PNG picture.

## The look

The pictures show the terminal in these colours, Gruvbox Material Hard
dark by sainnhe (<https://github.com/sainnhe/gruvbox-material>, MIT):

| | Colour | ratatui |
|---|---|---|
| background | `#1d2021` | `Color::Reset` as background |
| foreground | `#d4be98` | `Color::Reset` as text colour |
| 0 | `#1d2021` | `Black` |
| 1 | `#ea6962` | `Red` |
| 2 | `#a9b665` | `Green` |
| 3 | `#d8a657` | `Yellow` |
| 4 | `#7daea3` | `Blue` |
| 5 | `#d3869b` | `Magenta` |
| 6 | `#89b482` | `Cyan` |
| 7 | `#d4be98` | `Gray` |
| 8 | `#eddeb5` | `DarkGray` |
| 9 to 15 | as 1 to 7 | `LightRed` to `White` |

ratatui's named colours are the 16 ANSI colours: SGR 30-37 and 40-47 take
entries 0 to 7, 90-97 and 100-107 entries 8 to 15. `Color::Indexed` 0 to
15 takes the palette, 16 to 231 xterm's colour cube and 232 to 255 its
grey ramp; `Color::Rgb` stays as it is.

Reversed text swaps the text colour and the background. Dim text is drawn
at half opacity over its background, Ghostty's default `faint-opacity`.
Glyphs are blended over the pixels below in sRGB values, as Ghostty blends
on macOS by default. A border of 10 pixels in the background colour
surrounds the screen: a window padding of 5 points, as Ghostty draws it on
a display of content scale 2.

Box drawing, block elements and the sextants are painted with Ghostty's
geometry (`src/font/sprite/draw/` in ghostty-org/ghostty at commit
fbed180a5f18): partial blocks of `round(size * fraction)` pixels,
quadrants and sextants whose halves and thirds meet without a gap, lines
2 pixels wide (4 for heavy ones, double ones two light lines with a gap
of 2), dashes that tile from cell to cell, and arcs and diagonals as
Ghostty curves them. Ghostty is under the MIT licence, kept in
`LICENSE-ghostty`. The arcs and the diagonals are anti-aliased by
distance to their path, so their edge pixels may differ a little from
Ghostty's.

## Fonts and licences

| Directory | Files | Source | Licence |
|---|---|---|---|
| `fonts/victor-mono/` | `VictorMono-SemiBold.ttf`, `VictorMono-Bold.ttf`, `VictorMono-SemiBoldItalic.ttf`, `VictorMono-BoldItalic.ttf` | Victor Mono 1.560 by Rune Bjørnerås, `VictorMonoAll.zip` from <https://rubjo.github.io/victor-mono/> | SIL Open Font License 1.1, no Reserved Font Name (`LICENSE.txt`) |
| `fonts/dejavu/` | `DejaVuSansMono.ttf`, `DejaVuSansMono-Bold.ttf`, `DejaVuSans.ttf` | DejaVu Fonts 2.37, `dejavu-fonts-ttf-2.37.zip` from <https://github.com/dejavu-fonts/dejavu-fonts/releases> | Bitstream Vera and Arev licences; DejaVu's changes are in the public domain (`LICENSE`) |
| `fonts/noto-sans-symbols-2/` | `NotoSansSymbols2-Regular.ttf` (unhinted) | Noto Sans Symbols 2 2.008, `NotoSansSymbols2-v2.008.zip` from <https://github.com/notofonts/symbols/releases> | SIL Open Font License 1.1, no Reserved Font Name (`OFL.txt`) |

Victor Mono draws the text, in SemiBold for regular text, Bold for bold,
SemiBold Italic and Bold Italic for italics. It is the upstream release,
not the Nerd Fonts build: the Nerd Fonts build adds icon sets under
licences of their own, and asqr draws no Nerd Font icon.

What Victor Mono lacks comes from the next face that has it, in this
order:

1. DejaVu Sans Mono, Bold in bold text: the box marks `☐ ☑ ☒`, the checks
   `✓ ✔ ✗`, `✎`, the pointer `❯`, `⏎` and `U+FFFD`;
2. DejaVu Sans: `ℹ`;
3. Noto Sans Symbols 2: the Control Pictures `␀` to `␡`, which code and
   diff entries show for control bytes.

Box drawing (U+2500 to U+257F), block elements (U+2580 to U+259F) and the
sextants (U+1FB00 to U+1FB3B) come from no font: they are painted cell by
cell, as Ghostty draws them, so lines and Talqi's cells meet without
seams.

The test `tests/glyph_coverage.rs` checks that these fonts cover every
glyph asqr draws. It reads the characters of the string and character
literals in asqr's code, following its module tree from `src/lib.rs` and
`src/main.rs` and leaving out what only tests build, and of the files
that code includes, such as Talqi's templates. It adds what asqr computes
(the Control Pictures and `U+FFFD`), what ratatui draws for asqr's
rounded borders, and printable ASCII. It leaves out the Kitty image
placeholder `U+10EEEE` and its diacritics (`src/graphics/placeholders.rs`):
under the Kitty protocol the terminal draws the image where they stand,
so they are never drawn from a font.

## The cell

Every face is drawn at 28 pixels per em. A cell is 15 pixels wide and 39
high, with the baseline at row 30, the underline at rows 32 and 33, a
strikethrough at rows 21 and 22, and box-drawing lines 2 pixels wide (4
for heavy ones). These are the numbers Ghostty computes for the Nerd
Fonts build of Victor Mono at 14 pt on a display of content scale 2,
written down as constants in `src/fonts.rs` with the rules they follow.
The upstream file gives the same numbers, which a test checks.

Victor Mono's glyphs start at their cell's left edge with their own
bearings and are not clipped to the cell, so an italic may reach into the
next one, as in Ghostty. A glyph from a fallback face stands on the same
baseline, centred on its cells, and is scaled down about the baseline
when it is wider than its cells. A character of two columns takes two
cells and paints its background over both. Variation selectors and
joiners inside a cell draw nothing; a combining mark is drawn over the
character before it.
