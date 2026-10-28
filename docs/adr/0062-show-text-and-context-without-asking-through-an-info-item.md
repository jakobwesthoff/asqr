# 62. Show text and context without asking through an info item

Date: 2026-10-28

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md):
the question list marks an info item with its own mark

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
a fourth question kind, `info`, and its place in the result

## Context

- Every question kind asks for an answer: `single`, `multi` and `text`
  (ADR 28). An agent that wanted to show the person its results so far
  before its questions, such as a table of releases and what went wrong
  along the way, had to wrap them in a stand-in question with options
  such as "Passt" and "Rückfragen".
- The intro shows at most two lines under the title, and the rest only
  on `i` (ADR 47).
- New question kinds are added within format version 2 (ADR 61).

## Decision

- **The kind:** `info`. The name is short, fits `single`, `multi` and
  `text`, and says at once that nothing is answered.
- **Fields:** an info item has `id`, `header`, `text` and `context` like
  any question, and `note` works as on a question. `options` (an empty
  list too), an enabled `custom` and `length` are validation errors that
  say that an info item takes no answer and name the field to remove;
  `min` and `max` are refused as on every kind but `multi`. A session may
  consist of info items only, as a report the person can answer with
  notes.
- **Result:** an info item appears in `answers` only when the person
  wrote a note on it that is not blank, as `{"question": ..., "note":
  ...}`, in a submitted and in a cancelled result. There is no flag that
  says the person saw it: the result stays lean and carries no read
  signal, which would claim more than asqr knows.
- **TUI:** an info item has a tab like a question and counts neither as
  answered nor as skipped, so the review's counts, the confirmation of a
  submit and the session list leave it out. Its mark is a dim `ℹ`: there
  is nothing to do on it, so answered questions stand out more. The hint
  under its text reads "nothing to answer". It has one row, `next`, with
  the pointer like an option row, on which `enter` moves on: the item
  does not look empty or broken, and the row shows that `enter` moves
  on. The review shows its mark and `+note` for a note.

## Consequences

- asqr 0.13.1 and earlier refuse a session with an info item as not
  shaped like a session, and a queue they watch answers it with an error
  result.
- A text taller than the question's area is cut at the top, as on every
  question.
