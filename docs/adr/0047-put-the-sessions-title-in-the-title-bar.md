# 47. Put the session's title in the title bar

Date: 2026-10-16

## Status

Accepted

Amends [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md)

Amends [35. Render the text fields as full Markdown through patine](0035-render-the-text-fields-as-full-markdown-through-patine.md)

Amends [37. Colour the TUI by roles](0037-colour-the-tui-by-roles.md)

## Context

Above the tabs the screen showed four kinds of line without space
between them: the title bar with `asqr`, the queue and the count; the
session's title with `from` and `follows`; the intro, dimmed, in up to
three lines; then the tab bar. The area is squeezed, and little in it
registers except the questions.

The title line also cost the context pane its place. At 80×24, a
session whose title, `from` and `follows` id did not fit one line took
two lines for them and three for its intro. The tab's area kept 14
rows, one fewer than a context pane below the question needs (ADR 30),
and the pane shrank to its one-line hint.

## Decision

- **Title in the title bar:** the session's title stands next to `asqr`
  in the title bar. `from`, `follows` and the intro hang under it,
  indented to the title's first column, with a blank line before the
  tab bar. The intro takes at most two lines.
- **Intro in the text colour** instead of dim.
- **The header gives way** before the context pane drops to its hint:
  first its blank line, then intro lines.
- **The count is dim** beside a session, like the queue, rather than in the
  text colour ADR 37 gives the count in the title line.
- Where the title and the queue do not both fit, the queue goes before
  the title is cut. The title takes one line.
- The empty queue keeps its title bar as it was.
- The title starts in column 7, after ` asqr` and two blanks, and ends
  where the question's reading width ends. The queue and `N waiting`
  stand at the right end of the bar, a column before the edge; the
  queue shows whole or not at all, and the count always shows.
- `from` and `follows` share one dim line, `from <from> · follows
  <follows>`, cut with `…`.
- The header gives way while the tab's area would have fewer than 15
  rows, the fewest a context pane below the question needs, whatever
  the width and whether the session has context, so the header keeps
  its height from tab to tab. The intro keeps at least one line, ending
  in `… i: more`, and the `from` and `follows` line always stays.
- A title, `from` or `follows` with line breaks shows on one line with
  single blanks.

### The alternative: accent bar

An alternative is a bar `▎` in the primary colour left of the title,
the intro in the text colour and the `from` and `follows` line, with a
blank line above and below, while the title bar keeps the queue and the
count. It took 7 rows above the tabs at 110×32, lost the context pane to its hint at 80×24,
and needed a new use of the primary colour.

## Consequences

- At 80×24 the session described above keeps its context pane below the
  question: the header takes 4 rows and the tab's area 15.
- The intro shows at most two lines; `i` shows the rest.
- A session's title takes one line.
- A session without `from`, `follows` and intro has no header row on
  terminals of 20 rows or fewer, and its tab has that row.

## Amendment (2026-10-16, the count on the empty queue)

The count is dim on the empty queue too, so the queue and the count
are dim in every title bar. This replaces "The empty queue keeps its
title bar as it was." above; the empty queue's bar keeps its words and
its order.
