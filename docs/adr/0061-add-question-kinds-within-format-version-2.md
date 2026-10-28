# 61. Add question kinds within format version 2

Date: 2026-10-28

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
kinds are added within format version 2

Amends [51. Add context entry types within format version 2](0051-add-context-entry-types-within-format-version-2.md):
question kinds, which it left to ADR 28, are added the same way

## Context

- Format version 2 has three question kinds: `single`, `multi` and
  `text` (ADR 28). ADR 28 says that adding kinds needs a new format
  version or optional fields, and ADR 51 leaves question kinds to ADR 28.
- The parser reports a question of any other kind as an unknown variant
  and lists the kinds it expected, as in ``not a session file:
  questions[0].kind: unknown variant `rank`, expected one of `single`,
  `multi`, `text` ``, which does not say why the kind is unknown.
- The asqr that shows a queue runs as a long-lived process, so a session
  that a newer `asqr ask` accepted can reach an asqr that does not know
  its kind (ADR 51).

## Decision

- New question kinds are added within format version 2, as context entry
  types are (ADR 51).
- A question whose `kind` is a string this asqr does not know is refused
  with an error that names the kind as not supported by this asqr and its
  version, lists the kinds it supports, and says that a newer asqr may
  support it. An agent that wrote a session for a newer asqr gets an
  error that says what to do instead of the parser's shape error.
- The kind is checked on the raw JSON before the rest of the file, with
  the version and the entry types, and every such question is listed,
  its kind before the entries of its context. A missing `kind` or one
  that is no string stays a shape error.
- `asqr validate` and `asqr ask` refuse with the error, the dialog of an
  invalid session lists it, and the session's error result carries it.

## Consequences

- A session with a question of a kind that the asqr reading it does not
  know is refused as a whole, and gets an error result when it waits in
  a queue.
- A kind added within version 2 changes `schema/session.v2.json` under
  the same name, and a copy of an earlier one refuses a question of that
  kind, as an older asqr does.
- asqr 0.13.1 and earlier report a kind they do not know with the
  parser's `unknown variant`.
