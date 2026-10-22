---
title: "Explain that an unknown context type may need a newer asqr"
kind: improvement
component: format
origin: discussion
tags: [ux]
---
# Explain that an unknown context type may need a newer asqr

A context entry of a type asqr does not know makes the session invalid
with the parser's message alone. `asqr validate` on a session with
`{"type": "ansi", "content": "x"}` printed, on 2026-10-22 with 0.12.2:

```
error: not a session file: questions[0].context[0].type: unknown variant `ansi`, expected one of `image`, `code`, `markdown`, `diff`
```

and exited with 11. New context types are added within format version
2, so a type one asqr knows can be unknown to an older one, and the
message should say so.

## Proposal

Refuse an entry of an unknown `type` with a message that names the type
as not supported by this asqr's version, lists the supported types, and
says that a newer asqr may support it. `parse_session` in
`src/format/session.rs` already checks the raw JSON before the shape for
`type: "text"`, so that the error names the cause; an unknown type can
get its message the same way.

## When versions differ

The skill is bundled with asqr (`asqr skill`), so an agent and the asqr
it runs agree. The asqr that shows the queue runs as a long-lived
process: when asqr is updated while it runs, the new `asqr ask`
accepts a session with a new type, and the running older asqr reads it
from the queue and finds the type unknown.

## Done when

- `asqr validate` and `asqr ask` refuse an unknown context type with
  the message above.
- The dialog of an invalid session lists the same message, and the
  error result that asqr writes for the session carries it, so the
  agent learns it too (`reject` in `src/tui/inbox.rs`).
