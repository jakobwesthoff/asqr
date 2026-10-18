---
title: "Decide whether agents may turn off a question's note"
kind: investigation
component: format
status: needs-discussion
origin: request
tags: [ux]
---
# Decide whether agents may turn off a question's note

The user asked on 2026-10-15 whether it makes sense at all that an
agent can switch off the note of a question. Today every question takes
a note unless the session file sets `"note": false` on it (spec section
3.2, `note`, default `true`; skill: "set `"note": false` to turn it off").
`n` on such a question says that it takes no note.

## Context

- The note is the person's way to answer around the options: to give a
  condition, to say why they skip a question, or to say that the
  question is wrong. An agent that turns it off removes exactly that.
- The review tab has a note to the whole session since 0.11.0, which
  the person can always write. It covers remarks that span questions,
  but not a remark tied to one question.
- Format version 2 is released with 0.11.0, so the field cannot simply
  disappear: a session file that still sets it must keep working.

## Options

- Keep `"note": false` as it is.
- Ignore the field: every question takes a note; `"note": false` is
  read but has no effect, and `validate` warns that it is ignored.
- Drop the field in the next format version, with a warning for files
  that still set it.

## Open questions

- Is there a real case where a note on a question harms the agent or
  the person, for example a pure yes/no confirmation?
- If the field goes, does the skill say anything about notes beyond
  "read them"?
