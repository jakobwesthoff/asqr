# 68. Every item takes a note

Date: 2026-10-28

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
the asker can no longer turn a question's note off

Amends [36. Treat every typed field alike](0036-treat-every-typed-field-alike.md):
an open note stays open on a replaced file while its question shows

Amended by [102. Drop the note field from questions within format version 2](0102-drop-the-note-field-from-questions-within-format-version-2.md)

## Context

- A question's `note` field defaulted to `true`; `"note": false` turned
  the question's note off, its tab showed no notes line, and `n` said
  that the question takes no note (ADR 28). An info item took the field
  the same way (ADR 62).
- A note kept from a draft or from before a reload, on a question whose
  file turned notes off, was still sent but could not be seen or cleared
  on its tab.
- The note to the whole session on the review tab (ADR 29) covers a
  remark across questions, but not one tied to a question.
- Format version 2 is released (0.11.0), so session files that set the
  field must keep working.

## Decision

- Every question and every info item takes a note. Notes are among
  asqr's most useful features: through them the person sends remarks
  and information back to the asker at every point, so they are always
  on.
- `"note": false` is still read and has no effect. `asqr validate` and
  `asqr ask` warn that it is ignored, and the schema marks the field
  deprecated. A value that is no boolean stays a shape error.
- `asqr ask` removes `"note": false` from the copy it drops into the
  inbox, so an asqr of 0.13.1 or earlier that watches the queue offers
  the note too.
- An open note stays open on a replaced file while its question shows,
  also where its row is gone, as the note belongs to the question.
- The skill no longer offers the field; it says that the person can add
  a note to every question and info item, and that the asker reads every
  note.

## Consequences

- An asker cannot turn a note off.
- A session file that sets `"note": false` keeps working. One dropped
  into an inbox by hand, or queued before the upgrade, gets no warning,
  as the TUI shows none.
- A note typed in this asqr on a question whose file sets `"note":
  false` is sent but not shown by an asqr of 0.13.1 or earlier that
  resumes the draft.
