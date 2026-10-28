# 36. Treat every typed field alike

Date: 2026-10-14

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md)

Amends [29. Let the person add a note to the whole session](0029-let-the-person-add-a-note-to-the-whole-session.md):
the session note takes several lines

Amended by [66. Let the cursor keys reach every line of the question](0066-let-the-cursor-keys-reach-every-line-of-the-question.md)

Amended by [68. Every item takes a note](0068-every-item-takes-a-note.md)

## Context

asqr has five typed fields: the own answer, the answer of a `text`
question, a question's note, the note to the whole session and the
reject reason. One rule holds for every typed field, for three reasons:

- There is no reason for a field to refuse line breaks.
- One rule for every typed field: `enter` acts, `ctrl-j` adds a line.
- A pasted structured answer, such as a list, has to keep its structure
  in every field.

If the arrows only moved the text cursor in a field, a person could not
reach other questions without first pressing `esc`, and would not find
that key.

## Decision

- **Several lines everywhere:** every typed field takes line breaks.
  `ctrl-j` adds a line in every typed field, and every key bar of a field
  shows it.
- **`enter` keeps its action:** on the own answer it picks the own answer
  and moves on, in the session note it submits, in the reject reason it
  rejects, in a `text` answer it moves on, and in a question's note it
  returns to the row.
- **Paste:** a pasted text keeps its line breaks in every field. `\r`
  and `\r\n` become `\n`.
- **Results:** `custom`, the session `note`, a question's `note` and
  `reason` may contain `\n`. Validation never checks them for line
  breaks.
- **Review tab:** the summary line of a question shows a line break in
  the own answer or a `text` answer as `⏎` (spec section 7.8). The
  session note and the reject reason show their lines in their fields.
- **Wrapping:** a field's text wraps onto as many screen lines as it
  needs, focused or not, and is never cut with `…`. Further lines start
  under the start of the text, after the row's head. When the head
  leaves the text less than 20 columns, further lines start 4 columns
  in instead; the first line still starts after the head. The head holds
  the asker's label for the own answer.
- **Counter:** the counter of a focused field with a length stands on a
  line of its own after the text.
- **Rows as drawn:** `↑`/`↓` move the text cursor to the row above or
  below as the field shows it, which is the wrapped output, not the
  text's lines. A long text without line breaks is one line but several
  rows, and `↑` moves up a row.
- **Leaving:** `←` with the text cursor at the start of a field's text
  goes to the previous question, `→` at the end to the next one. Inside
  the text the arrows move the cursor, and `esc` leaves the field. A
  field is left with `↑` only from its first row and with `↓` only from
  its last row. A question's note, which is not a row, keeps the cursor
  where it is there. The help lists these rules; the key bar inside a
  field does not, as it is full at 60 columns.
- **The column:** the cursor goes to the same display column in the row
  it moves to, or to the end of a shorter row. While `↑` and `↓` follow
  each other, the column they started from is kept, so passing a short
  row does not lose it.
- **One component:** every typed field is the same component,
  `src/tui/text_field.rs`, for editing, measuring and drawing. What
  depends on the field, such as where its text is kept and what `enter`
  means, stays with the session state.
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
- **The end of a full row,** the one exception: `↓` from the last row
  of a line whose end shows on a row of its own goes to that end, and
  only the next `↓` moves on or leaves the field. `↑` from a line below
  goes to the full row. So a person who typed to the end of a full row,
  pressed `↑` and then `↓` is back at the end instead of out of the
  field.

Every field hands the keys it does not handle itself to its editor,
ratatui-textarea, whose default key table assumes that the editor draws
itself. asqr draws the field, so each key does the following in a field
(spec section 7.7):

- **Kept:** the editor's moves by character, word, line, paragraph and
  to the first or last line, its deletes, undo, redo and `ctrl-y`.
- **Mapped:** `ctrl-p` and `ctrl-n` move by a row as `↑` and `↓` do,
  leaving the field from its first or last row.
- **Swallowed:** `pgup`/`pgdn`, `ctrl-v` and `alt-v` never reach the
  editor, whose own view never has a size, so they would move the text
  cursor to the start of the text. `pgup`/`pgdn` scroll the context pane
  where one shows (ADR 31); otherwise they, `ctrl-v` and `alt-v` do
  nothing.
- **Never typed:** `shift-tab` acts as `tab` everywhere, and neither
  reaches the editor, which would insert a tab character that shows as
  nothing.
- **No selection:** a move with `shift` held would start a selection the
  field does not show, and the next typed key would replace the selected
  text. The selection ends after every key, so `shift` only moves and
  `ctrl-x` cuts nothing.
- **Pasted control characters:** a field draws no tab and no other
  control character, so a pasted one would be text the person cannot
  see, and a position where the text cursor has no cell. A paste turns a
  tab into blanks up to the next multiple of 4 columns and drops the
  other control characters. Text from a draft is cleaned the same way
  when it is restored, so a field never counts rows it does not draw.
- **A paste where no field has the keys** goes into the field of the
  row, giving it the keys again, and elsewhere a message says where a
  paste goes.
- **Entering a field by direction:** the cursor that moves down onto a
  field's row puts the text cursor at the end of the field's first row,
  not at the end of the text, where `↓` into a field of several rows
  would land on its last row, past the rows in between. Moving up onto
  it puts the text cursor at the end of its last row, which is the end
  of the text. A field of one row is entered at the end of its text
  either way. The rows are the rows as drawn. A field entered from
  above is drawn once with the text cursor at the end of its first
  line, as its rows are known only then; the cursor then moves to its
  first row's end and the screen is drawn again at once, unless a key
  came first. `esc` followed by `↑` or `↓` on the same row, a switch
  between questions and a paste keep the end of the text.
- **A replaced session file** keeps the focus on the row found again: a
  field left with `esc` stays without the keys, a field typed in keeps
  its text cursor, and an open note stays open while the question still
  takes notes.

## Consequences

- A person can paste or type a structured answer into any field, and the
  asker gets it with its lines.
- An agent reading `custom`, `note` or `reason` has to expect line
  breaks in each of them. The skill says so.
- The cursor reaches every line of a multi-line `text` answer and every
  row of a long own answer with the arrows, and a person can move
  through every question with the arrows alone.
- `↑` and `↓` take as many presses as a field has rows before they leave
  it.
- The row of a long own answer takes several lines. The question scrolls
  so that the whole row stays in view. When the row is taller than the
  area, its start is shown, and the text cursor at the end of a long
  text can lie below the visible lines.
- A field never takes a typed tab character.
