# 36. Treat every typed field alike

Date: 2026-10-14

## Status

Accepted

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

Amends [27. Wrap the own answer and the reject reason](0027-wrap-the-own-answer-and-the-reject-reason.md)

## Context

asqr has five typed fields: the own answer, the answer of a `text`
question, a question's note, the note to the whole session and the
reject reason. They followed different rules for `↑`/`↓`:

- In the own answer, the `text` answer, the session note and the reject
  reason, `↑`/`↓` left the row at once (ADR 22), from any of the rows a
  long text wraps onto (ADR 27). A `text` question has only one row, so
  there `↑` did nothing at all: the cursor could not reach an earlier
  line of a multi-line answer.
- In a question's note, `↑`/`↓` moved the text cursor between the
  note's lines, as the editor does.

The user decided on 2026-10-14 that one rule holds for every typed
field.

## Decision

- **Rows as drawn:** `↑`/`↓` move the text cursor to the row above or
  below as the field shows it, which is the wrapped output, not the
  text's lines. A long text without line breaks is one line but several
  rows, and `↑` moves up a row (user, 2026-10-14).
- **Leaving:** a field is left with `↑` only from its first row and with
  `↓` only from its last row, as `←` leaves only at the start of the
  text and `→` only at its end. A question's note, which is not a row,
  keeps the cursor where it is there.
- **The column:** the cursor goes to the same display column in the row
  it moves to, or to the end of a shorter row. While `↑` and `↓` follow
  each other, the column they started from is kept, so passing a short
  row does not lose it.
- **Key bars:** the key bars of the own answer, the session note and
  the reject reason drop their `↑/↓ leave` and `↑ back` hints, which no
  longer held. The help states the rule.

Agent's choices within that design:

- **One layout:** drawing a field, counting its rows for the layout
  below a context pane (ADR 30), placing its text cursor and moving
  with `↑`/`↓` go through one function in `src/tui/text_field.rs`,
  which breaks the text into rows and knows which characters each row
  shows. They cannot disagree about where a row breaks.
- **The width drawn:** `↑`/`↓` use the head and width of the field's
  last draw, as a key acts on what is on screen. The screen hands them
  to the session after each draw. Before the first draw the rows are
  the text's lines.
- **Rows the cursor changes:** the cursor at the end of a line whose
  last row fills the width shows on a row of its own, and so does the
  cursor on the blank where such a row breaks. `↑`/`↓` aim for a row on
  screen and never move the cursor to a position that would show on
  another row; they take the closest position before it on that row.
- **The end of a full row,** the one exception: `↓` from the last row of a line whose end
  shows on a row of its own goes to that end, and only the next `↓`
  moves on or leaves the field. `↑` from a line below goes to the full
  row. So a person who typed to the end of a full row, pressed `↑` and
  then `↓` is back at the end instead of out of the field (user,
  2026-10-14).

## Consequences

- The cursor reaches every line of a multi-line `text` answer and every
  row of a long own answer with the arrows.
- `↑` and `↓` take as many presses as a field has rows before they leave
  it.
