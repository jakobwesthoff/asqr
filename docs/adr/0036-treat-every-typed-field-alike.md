# 36. Treat every typed field alike

Date: 2026-10-14

## Status

Accepted

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

Amends [27. Wrap the own answer and the reject reason](0027-wrap-the-own-answer-and-the-reject-reason.md)

Supersedes the one-line own answer of [7. Session and result format version 1](0007-session-and-result-format-version-1.md)

Supersedes the one-line own answer of [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

Supersedes the one-line value of [27. Wrap the own answer and the reject reason](0027-wrap-the-own-answer-and-the-reject-reason.md)

Supersedes the one-line session note of [29. Let the person add a note to the whole session](0029-let-the-person-add-a-note-to-the-whole-session.md)

## Context

asqr has five typed fields: the own answer, the answer of a `text`
question, a question's note, the note to the whole session and the
reject reason. They followed different rules.

Two of them took several lines: the `text` answer and the question's
note, where `ctrl-j` added a line. The own answer, the session note and
the reject reason were one line (ADRs 7, 22, 27 and 29). There, `ctrl-j`
did nothing, and a pasted line break became a blank.

They also differed in `↑`/`↓`:

- In the own answer, the `text` answer, the session note and the reject
  reason, `↑`/`↓` left the row at once (ADR 22), from any of the rows a
  long text wraps onto (ADR 27). A `text` question has only one row, so
  there `↑` did nothing at all: the cursor could not reach an earlier
  line of a multi-line answer.
- In a question's note, `↑`/`↓` moved the text cursor between the
  note's lines, as the editor does.

The user decided on 2026-10-14 that one rule holds for every typed
field, for three reasons:

- There is no reason for a field to refuse line breaks.
- One rule for every typed field: `enter` acts, `ctrl-j` adds a line.
- A pasted structured answer, such as a list, kept its structure only in
  some fields and lost it in the others.

## Decision

- **Several lines everywhere:** the own answer, the session note and the
  reject reason take line breaks like the `text` answer and the
  question's note. `ctrl-j` adds a line in every typed field, and every
  key bar of a field shows it.
- **`enter` keeps its action:** on the own answer it picks the own answer
  and moves on, in the session note it submits, in the reject reason it
  rejects, in a `text` answer it moves on, and in a question's note it
  returns to the row.
- **Paste:** a pasted text keeps its line breaks in every field. `\r`
  and `\r\n` still become `\n`.
- **Results:** `custom`, the session `note` and `reason` may contain
  `\n`, as `custom` of a `text` answer and a question's `note` already
  could. The format stays version 2: the fields keep their names and
  types, and validation never checked them for line breaks.
- **Review tab:** the summary line of a question shows a line break in
  the own answer as `⏎`, as it does for a `text` answer (spec section
  7.4). The session note and the reject reason show their lines in their
  fields.
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
- **The end of a full row,** the one exception: `↓` from the last row of a line whose end
  shows on a row of its own goes to that end, and only the next `↓`
  moves on or leaves the field. `↑` from a line below goes to the full
  row. So a person who typed to the end of a full row, pressed `↑` and
  then `↓` is back at the end instead of out of the field (user,
  2026-10-14).

## Consequences

- A person can paste or type a structured answer into any field, and the
  asker gets it with its lines.
- An agent reading `custom`, `note` or `reason` has to expect line
  breaks in each of them. The skill says so.
- The cursor reaches every line of a multi-line `text` answer and every
  row of a long own answer with the arrows.
- `↑` and `↓` take as many presses as a field has rows before they leave
  it.

## Amendment (2026-10-14, adversarial review round 2)

Every field hands the keys it does not handle itself to its editor,
ratatui-textarea, whose default key table assumes that the editor draws
itself. asqr draws the field, so some of those keys acted on a view
nobody saw. The lead and the reviewer agreed on what each key does in a
field (spec section 7.3):

- **Kept:** the editor's moves by character, word, line, paragraph and
  to the first or last line, its deletes, undo, redo and `ctrl-y`.
- **Mapped:** `ctrl-p` and `ctrl-n` move by a row as `↑` and `↓` do,
  leaving the field from its first or last row.
- **Swallowed:** `pgup`/`pgdn`, `ctrl-v` and `alt-v` scroll the editor's
  own view, which never has a size, and so moved the text cursor to the
  start of the text. Without a context pane they do nothing.
- **Never typed:** `shift-tab` acts as `tab` everywhere, and neither
  reaches the editor, which would insert a tab character that shows as
  nothing.
- **No selection:** a move with `shift` held started a selection the
  field did not show, and the next typed key replaced the selected text.
  The selection ends after every key, so `shift` only moves and `ctrl-x`
  cuts nothing.
- **Pasted control characters:** a field draws no tab and no other
  control character, so a pasted one was text the person could not see,
  and a position where the text cursor had no cell. A paste turns a tab
  into blanks up to the next multiple of 4 columns and drops the other
  control characters. Text from a draft is cleaned the same way when it
  is restored, so a field never counts rows it does not draw.
- **A paste where no field has the keys** goes into the field of the
  row, giving it the keys again, and elsewhere a message says where a
  paste goes, where it vanished before.
