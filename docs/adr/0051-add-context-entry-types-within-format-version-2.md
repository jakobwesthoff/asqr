# 51. Add context entry types within format version 2

Date: 2026-10-22

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md)

## Context

Format version 2 has four context entry types: `image`, `code`,
`markdown` and `diff` (ADR 28). The parser reports an entry of any other
type as an unknown variant and lists the types it expected, which does
not say why a type is unknown.

`asqr skill` prints the skill of the asqr that runs it, so an agent that
reads it there and the asqr it runs agree on the format. The asqr that
shows a queue runs as a long-lived process: when asqr is updated while
it runs, the new `asqr ask` accepts a session that the running asqr
reads from the queue and cannot parse.

## Decision

- New context entry types are added within format version 2, so an
  asqr can meet an entry of a type a newer asqr knows.
- An entry whose `type` is a string this asqr does not know is refused
  with an error that names the type as not supported by this asqr and
  its version, lists the types it supports, and says that a newer asqr
  may support it.
- An entry of type `text` keeps its own error, which says which of
  `code` and `markdown` to pick (ADR 28).
- Both are checked on the raw JSON before the rest of the file, as the
  version is, and every such entry is listed. A missing `type` or one
  that is no string stays a shape error.
- `asqr validate` and `asqr ask` refuse with the error, the dialog of
  an invalid session lists it, and the session's error result carries
  it, so the asker learns it too.

## Consequences

- Adding a context entry type needs no new format version. Question
  kinds and other fields follow ADR 28.
- A session with an entry of a type that the asqr reading it does not
  know is refused as a whole, and gets an error result when it waits in
  a queue; the entry is not skipped.
- The session schema is derived from the types, so a type added within
  version 2 changes `schema/session.v2.json` under the same name, and a
  copy of an earlier one refuses an entry of that type, as an older
  asqr does.
