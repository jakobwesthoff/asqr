# 102. Drop the note field from questions within format version 2

Date: 2026-10-31

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
a question and an info item have no `note` field

Amends [68. Every item takes a note](0068-every-item-takes-a-note.md):
the field is no longer read or marked deprecated, a `note` that is no
boolean is no longer a shape error, the warning applies to every value,
and `asqr ask` removes the field whatever its value

## Context

- ADR 68 made every question and info item take a note and left the
  `note` field in the format, read and ignored, so that released session
  files stay valid.
- Unknown fields are ignored and reported in every format version (ADR 28,
  spec section 3.4), so a file that sets a field asqr does not have stays
  valid without a new version.
- A released asqr reads `note` as a boolean. It turns the note off for
  `false` (0.13.1 and earlier) and refuses a session where the value is no
  boolean, `null` included (0.14.0 and 0.15.0 too).
- An asqr that watches the queue keeps running when asqr is updated, so it
  can meet a session that the newer `asqr ask` accepted (spec section 5.8).

## Decision

- The question and info item types, the schema and the spec's field list
  have no `note` field. Format version 2 stays version 2.
- A `note` of any value on a question or an info item, `null` included,
  is ignored and gets its own warning from `asqr validate` and `asqr ask`:
  `questions[0].note: ignored; every question takes a note`. It replaces
  the generic unknown-field warning for this field on a question. A `note`
  on an option or on the session stays an unknown field.
- A `note` that is no boolean is valid. It was a shape error.
- `asqr ask` removes `note` from the questions and info items of the copy
  it puts into the inbox, whatever its value. ADR 68 removed `false` only.
- The skill no longer mentions the field. It says that the person can add
  a note to every question and info item.

## Alternatives rejected

- A format version 3 that reads version 2 as well.
- A format version 3 that refuses version 2: it would refuse every queued
  session and every agent with an older skill.
- The generic unknown-field warning for `note`.
- Refusing a file that sets `note`.

## Consequences

- `"note": true` gets a warning. It got none before.
- The warning, its `WarningKind::NoteField` and `drop_note` in
  `src/cli/ask.rs` carry a TODO: they can go once asqr moves to a format
  version 3, as a released asqr refuses a file of another version before it
  reads its fields.
- A session file dropped into an inbox by hand with a `note` that is no
  boolean reaches a running asqr of a released version unchanged and gets
  an error result there.
