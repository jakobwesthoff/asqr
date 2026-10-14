---
title: "Let the person add a note to the whole session on the review tab"
kind: feature
component: tui
horizon: next
origin: request
tags: [ux, milestone/v0-11]
---
# Let the person add a note to the whole session on the review tab

The user asked for this on 2026-10-14. Every question takes a note
today, but the session as a whole does not. A note on the review tab
lets a submitted session carry extra information back to the agent, for
example an approval with a condition, or a remark that spans several
questions.

## Where asqr stands

- The review tab (spec section 7.4, ADR 0022) lists every question with
  its answer and ends with the submit and reject rows. The reject row
  has a one-line reason field, which only a rejected session returns.
- A question's note is edited after `n`, is multi-line (`ctrl-j`), and
  lands in the answer's `note` field of the result.
- The result (format version 1, `schema/result.v1.json`) has no
  session-level note; a cancelled result has `reason`.

## Proposal

- `n` on the review tab edits a session note, shown and edited like a
  question's note, in its own row above the submit row.
- The result carries it as a top-level `note` on a submitted session.
  On a rejected session the reject reason already covers it.
- The field belongs to result format version 2, which the context pane
  plan introduces in 0.11.0 together with session format version 2
  (user, 2026-10-14). It ships in the same release, and the format step
  of that plan names it as its first new result field.
- The draft keeps it like every other edit.
- Spec, schema, ADR and the agent skill (how to read the result) are
  updated with it.

## Open questions

- Whether a rejected session also returns the note next to its reason,
  or the two merge.
