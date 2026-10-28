# 60. Highlight a diff's unchanged lines and fill its changed ones

Date: 2026-10-28

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
a diff entry takes an optional `language`

Amends [33. Show diffs as a unified diff computed with similar](0033-show-diffs-as-a-unified-diff-computed-with-similar.md):
unchanged lines are highlighted, changed lines are filled

Amends [37. Colour the TUI by roles](0037-colour-the-tui-by-roles.md):
the fills of changed diff lines, and the success and error rows for added
and removed diff lines

Amended by [70. Show a diff side by side on a key](0070-show-a-diff-side-by-side-on-a-key.md):
side by side, a changed line fills its column

## Context

Code entries are highlighted in their language (ADR 34); diffs show
without syntax highlighting, removed lines in red text and added lines in
green text (ADR 33). In a review, a suggested change is the entry the
person reads most. A highlighted code block of the new version shows the
colours but not what the change replaces.

patine highlights with the 8-colour ANSI theme (ADR 34): comments and
strings in green, types and numbers in yellow, functions in blue,
keywords in magenta, parameters in cyan, and HTML tags in red. The
contrast of these colours against the terminal's red and green, measured
by WCAG 2's relative luminance over the 607 themes Ghostty ships:

| Text | On red | On green |
|---|---|---|
| every syntax colour other than the fill's own, at 3:1 | 0% of themes | 0% of themes |
| the theme's background colour, at 3:1 | 85% | 92% |
| the theme's background colour, at 4.5:1 | 57% | 76% |

On the theme's own background, as a code entry shows them, all six
syntax colours reach 3:1 in 65% of these themes and 4.5:1 in 38%.

## Decision

- **The language:** a diff entry takes an optional `language`, such as
  `tsx`. Without it, the extension of `name` decides, or the whole name
  when it has none, as `Makefile` and `Dockerfile` do for code entries.
  The `title` and the paths of the sides do not decide. An empty
  `language` counts as given, as for a code entry. Unknown fields are
  ignored when parsing, so an asqr that does not know the key shows the
  diff as before; the format stays version 2.
- **The diff comes first, syntax colours are an extra:** the change has
  to be obvious at once.
- **Changed lines are filled:** a removed line fills its row red and an
  added line green, across the pane's width, its numbers and sign
  included: reversed video of the terminal's red and green, so the theme
  decides the shades and the text shows in the theme's background colour.
  Dim is lifted in the fill, and the `‹` and `›` of a cut line take the
  fill's colour, as dim text does under the focus fill. A diff without a
  language has the same fills.
- **Changed lines carry no syntax colours:** on the terminal's red or
  green, no syntax colour reaches 3:1 in any of the measured themes,
  while the theme's background colour does in 85% and 92% of them.
- **Unchanged lines are highlighted** in the diff's language, as a code
  entry is, and show plain without one.
- **The new text, from its first line:** unchanged lines take their
  colours from the new text, highlighted as one from its first line to
  the last unchanged line the hunks show. The lexer state carries from
  line to line, so a hunk that starts inside a block comment, a template
  string or a multi-line string shows it as one; a hunk highlighted
  alone would start in the grammar's first state and colour the inside
  of a comment as code. The state at a line depends only on the text
  above it, so nothing after the last unchanged line shown is
  highlighted. A new or deleted file, all of whose lines are changed,
  shows no syntax colours and highlights nothing.
- **At most 1 MiB:** a diff whose two sides together are longer than
  1 MiB shows without highlighting, as a code entry above 1 MiB does. The
  sides count as read, as the size warning counts them.
- **What stays from ADR 33:** the terminal's own colours, the signs, the
  gutter, and nothing marked inside a line: a changed row is one fill
  from edge to edge.
- **The fills of ADR 37:** the focus fill and the submit button were the
  only fills asqr draws itself (an `ansi` entry's reversed text is the
  asker's). The changed lines of a diff join them, red as the error
  colour and green as the success colour. Without colours a changed line
  stays a reversed row. It stays apart from the focus fill by its line
  numbers and its `-` or `+`, and the focus fill never falls on a row of
  an entry's body.

## Consequences

- A diff entry no longer gets an unknown-field warning for `language`.
- A diff of a new file shows as green rows without syntax colours; a
  `code` entry shows new code highlighted.
- Every diff's changed lines look different from before, with or
  without a language.
