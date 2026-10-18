# 27. Wrap the own answer and the reject reason

Date: 2026-10-14

## Status

Accepted

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

Amended by [36. Treat every typed field alike](0036-treat-every-typed-field-alike.md)

One-line value superseded by [36. Treat every typed field alike](0036-treat-every-typed-field-alike.md)

## Context

ADR 22 made the own answer one line that scrolls sideways, and the
reject reason a one-line field like it. While typed, a long own answer
showed only a window of its text that ended at the text cursor, with
`…` for text scrolled out on the left. Without the focus, it was cut at
the end of the line with `…`. A note wraps onto further lines under its
start.

## Decision

- **Wrapping:** the own answer's text wraps onto as many screen lines as
  it needs, focused or not, and is never cut with `…`. Further lines
  start under the start of the text, after the row's head.
- **Wide heads:** when the head leaves the text less than 20 columns,
  further lines start 4 columns in instead. The first line still starts
  after the head. The head holds the asker's label for the own answer.
- **One value line:** the value stays one line. `ctrl-j` adds no line
  break, and the format keeps own answers to one line (ADR 7). `↑`/`↓`
  leave the row from any of its screen lines.
- **Counter:** the counter of a focused field moves to a line of its own
  after the text, as with a `text` answer.
- **Reject reason:** it wraps the same way.

## Consequences

- A long own answer or reject reason wraps instead of being cut, in
  every width from 60 columns on.
- The row of a long own answer takes several lines. The question scrolls
  so that the whole row stays in view. When the row is taller than the
  area, its start is shown, and the text cursor at the end of a long
  text can lie below the visible lines, as with a `text` answer.
