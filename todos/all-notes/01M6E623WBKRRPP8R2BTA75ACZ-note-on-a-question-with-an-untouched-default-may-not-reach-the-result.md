---
title: "Note on a question with an untouched default may not reach the result"
kind: bug
component: tui
origin: incident
tags: [ux]
---
# Note on a question with an untouched default may not reach the result

On 2026-11-02 Jakob answered an agent's session "Where does the Dachser
backup go in Google Drive?" (reference `7T0YVJVV`, id
`01M6E5CSZ5Z0J69GHZSM3K8M1E`). He is sure he wrote a note on its first
question (a `single` question whose first option carried
`default: true`) saying the backup should go to another place than the
preselected option. The stored result has no such note: that question
comes back as `selected: ["kunden"]`, the default option, with no
`note`, no `custom` and no `defaulted` flag, and the session has no
top-level note. A note on another question of the same session ("what")
did arrive. The agent acted on the default and Jakob had to correct it
later.

## Problem

Not reproduced; only the one occurrence above. Jakob's own reading is
that he may have typed the note and then not confirmed the preselected
default with Enter before moving on. It is equally possible that he
never typed it. Two things in the result are worth checking against the
code:

- whether a note typed on a question whose default is left untouched
  (no Enter on an option) is kept in the draft and written to the
  result;
- whether `defaulted: true` is set when the person leaves the default
  as it is. Here the result reported the default option as a plain
  selection, which by the result format means he picked it actively.
  If he did not press Enter on it, the flag is missing.

## Impact

If either happens, an agent reads an untouched default as the person's
decision and loses the note that said otherwise. That is the worst
place to lose input, since the note exists to override exactly that
default.

## To test

Build a session with a `single` question that has a default and a
second question. Then, in `asqr watch`:

1. On the first question write a note without touching the options,
   move on with the usual key, answer the second question, submit.
2. The same, but move on to the review tab directly after the note.
3. The same, but pick the default explicitly with Enter after the note.

For each, compare the result JSON: the note must be there, and
`defaulted: true` must be set in 1 and 2 and absent in 3. The session
above is in the archive (`asqr replay 7T0YVJVV`) and can serve as the
test file.

## Done when

The three cases are tested. Either the result is correct in all of
them (then this todo is closed with that finding), or the failing case
is fixed with a test that covers it.
