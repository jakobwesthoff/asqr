# 66. Let the cursor keys reach every line of the question

Date: 2026-10-28

## Status

Accepted

Amends [55. Wrap option labels and render them as Markdown](0055-wrap-option-labels-and-render-them-as-markdown.md)

Amends [36. Treat every typed field alike](0036-treat-every-typed-field-alike.md)

## Context

The question's area scrolls so the row under the cursor shows. An option
taller than the area shows from its start (ADR 55): its later lines show
only once the cursor is on a later row, the middle lines of an option
taller than about twice the area never show, and the last lines of the
last option, when no row follows it, never show. The first line of such
an option stands on the area's first row under `↑ more`, which covers its
end where that is not blank.

A question text taller than what the area leaves above the first row is
cut off at its start, and `↑` on the first row does nothing. Lines below
the last row, such as a note of several lines, are cut off the same way.
All of it takes a small terminal.

## Decision

The keys that move the cursor reach every line of the question's area.
Text the person decides on must never be out of reach, for every
question.

- For the row under the cursor, the area can take a range of positions.
  `↓`/`j` and `↑`/`k` first move the area through that range a line at a
  time, and only at its end move the cursor as before.
- On the first row the range runs up to the area's first line, so `↑`
  scrolls through the question text until its start shows and `↓`
  scrolls back before the cursor moves on. On the last row it runs down
  to the last line below it.
- A row other than a field that does not fit the area together with the
  line above it and the line below it scrolls through all of them, so
  `↑ more` and `↓ more` cover those neighbours, not the ends of its own
  first and last lines. Moving up onto it shows its last lines and the
  line below it, as a field entered from below shows its end (ADR 36);
  any other way onto it shows the line above it and its first lines. It
  behaves as long text does elsewhere: a field taller than the area
  scrolls through its rows with the arrows. This replaces the Height
  bullet of ADR 55.
- A field on the first or last row takes part with `↑`/`↓` and
  `ctrl-p`/`ctrl-n` at its first and last row. While the area is
  scrolled away from it, those keys scroll back toward it before they
  move its text cursor, and any other key or paste the field takes shows
  it again at once. Fields keep their own view otherwise: a field never
  scrolls through its rows by this rule, and one left with `esc` shows
  its start as before.
- Only these keys move the area. Every other key that acts on the
  question, such as `n`, `space`, a digit, `enter` or `backspace`, and a
  paste, first put it back where the row under the cursor shows. `?`,
  `L`, `i`, `o`, `z`, `s` and `q` act on no row and leave it where it
  is, as do the keys the context pane takes. While a question's note has
  the keys, the area keeps the note's text cursor in view.

In ADR 36, "The question scrolls so that the whole row stays in view"
holds except while the lines above the first row or below the last row
are scrolled into view.

## Consequences

- The keys take one press per line the area can move before the cursor
  moves, the neighbour lines of a tall option included.
- On the first or last row, the row under the cursor can be scrolled out
  of view, until a key other than these brings it back.
- The key bar and the help are unchanged.
