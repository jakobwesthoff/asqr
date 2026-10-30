# screens

A development tool of the asqr repository, not published: the pictures
of asqr's documentation are drawn with asqr's own code
([ADR 89](../../docs/adr/0089-draw-the-documentation-pictures-with-asqrs-own-rendering.md)).
This crate holds the fonts the pictures are drawn with, and their
licences.

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
