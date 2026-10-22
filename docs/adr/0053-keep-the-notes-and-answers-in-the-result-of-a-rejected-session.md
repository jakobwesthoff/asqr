# 53. Keep the notes and answers in the result of a rejected session

Date: 2026-10-22

## Status

Accepted

Amends [13. Let the person reject a session](0013-let-the-person-reject-a-session.md)

Amends [29. Let the person add a note to the whole session](0029-let-the-person-add-a-note-to-the-whole-session.md)

## Context

A rejected result carried only the status `cancelled` and the reason.
The notes and answers typed before rejecting were dropped, and the
finish deletes the draft, so the asker could not get them back. A
reason that refers to a note, the natural way to say "see what I wrote
there", reached the asker without that note.

## Decision

- A `cancelled` result carries the session note when there is one,
  trimmed and absent when blank, as a submitted result does.
- Its `answers` hold the questions with an answer or with a note that
  is not blank, in session order. A skipped question without a note is
  left out.
- An untouched default counts as an answer here as it does in a submit
  (spec section 4.2), and carries `defaulted: true`. Without it, the
  asker could not tell a default the person kept from one they removed
  and left empty; the flag says the person did not touch it.
- The answers take the shape of a submitted result, which keeps only
  what counts (spec section 4.2): own-answer text typed next to a chosen
  option of a `single` question is not in it, as in a submit.
- No flag marks these answers as partial: `status: cancelled` already
  says that nothing was submitted.
- Rejecting stays one step, without a warning.
- The result stays in format version 2.

## Consequences

- An asker reading a `cancelled` result looks at `note` and `answers`
  next to `reason`. The skill says so.
