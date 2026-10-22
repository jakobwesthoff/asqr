# 54. Set the context pane off from the question with a ledge

Date: 2026-10-22

## Status

Accepted

Amends [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md):
beside the question the pane has a left edge, and below it a blank row
stands between question and pane

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md):
the header is a rule with the entry's name set into it, and the position
moves into a bottom rule

Amends [37. Colour the TUI by roles](0037-colour-the-tui-by-roles.md):
the focused pane fills its name and turns its lines yellow

## Context

Beside the question, the context pane starts one blank column right of
it (ADR 30), and its header (ADR 31) names the entry in bold in the text
colour (ADR 37), like the Markdown headings under it. The question's
lines end at the blank column and the pane's lines start right after it,
so the two sides read as one line, and nothing marks where the pane
begins.

## Decision

The pane stands on a ledge: its header is a dim rule with the entry's
name set into it, which turns down into a dim left edge, and a bottom
rule with the position closes it.

- **Header:** a dim rule with the entry's name in bold, one blank on
  each side of it, the rule running on to the pane's right edge:
  `╭─ name ─────` beside the question, `── name ─────` below it. The
  name gets the header's width less the rule's pieces and is cut with
  `…` where it is longer.
- **Left edge:** beside the question, a dim `│` runs down the pane's
  first column from the `╭` to the bottom rule. The content starts two
  columns right of it. The blank column between question and pane stays,
  so the question keeps its width.
- **Below the question** there is no left edge, and the content starts
  at the area's left. The context area's first row stays blank, so a
  blank row stands between the question and the header rule.
- **Bottom rule:** the pane's last row is a rule that carries the
  position ADR 31 put at the header's right end, dim: `╰──── 1–31 of 58
  · tab ─` beside the question, `──── 1–31 of 58 · tab ──` below it,
  with `· tab` while the pane does not have the focus, or `tab` alone
  for an entry that is only wider than the pane. Where the rule has no
  room for `· tab`, it goes. When the content fits and there is no
  position, the bottom rule stays, without text, running solid to the
  pane's right edge, so the pane keeps its shape and its body the same
  height from entry to entry.
- **Focus:** the focused pane's name takes the focus fill, its blanks
  included, and its header rule, left edge and bottom rule turn yellow.
  The position stays dim. This replaces the focus fill over the whole
  header row.
- **Mark:** the red `✗` of an entry asqr cannot show (ADR 49) stands in
  the blank before the name, `╭─ ✗ name ─────`. Under the focus fill it
  takes the fill with the name.
- **Full screen:** the full-screen view (`z`) takes the ledge in the
  form it has beside the question: `╭`, left edge and `╰`.
- A row without context leaves the whole pane empty, its ledge included,
  as ADR 31 has it.
- `MIN_CONTEXT_WIDTH` and the width from which beside fits stay as ADR
  30 sets them, and so does the 8-row context area below the question.

## Consequences

- Beside the question, and full screen, the pane's body is two columns
  narrower than the pane, 38 columns at the narrowest.
- The bottom rule takes the body's last row in both arrangements, and
  below the question the blank row takes one more: beside the question
  the body is two rows lower than the pane, below it three, so the
  smallest pane below the question, 8 rows, has a body of 5 rows.
- The card of an entry asqr cannot show keeps its own rounded frame
  inside the ledge.
- Under `NO_COLOR` the lines lose their yellow: the focused pane's lines
  show in the text colour, an unfocused pane's dim, and the reversed
  fill on the name alone shows the focus.
- The rules change the text of the screen, not only its colours.
