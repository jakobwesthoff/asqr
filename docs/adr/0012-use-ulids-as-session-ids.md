# 12. Use ULIDs as session ids

Date: 2026-09-27

## Status

Accepted

## Context

Session ids name the result files and must be unique per queue. Askers
should not have to invent ids, and a reused id must not destroy an
answer nobody has read yet.

## Decision

- Ids asqr creates are ULIDs: session ids it assigns, and ids for
  internal use such as drafts and locks.
- The session `id` is optional. Without one, `asqr ask` assigns a fresh
  ULID and prints it.
- `asqr new` prints a session skeleton with a fresh ULID.
- A custom id in the allowed syntax (letters, digits, `-`, `_`, `.`) is
  still accepted.
- An id whose earlier session was answered, with its result still in the
  outbox, is rejected. `asqr ask --force` moves the old result into the
  archive and accepts the new session.

Rejected: requiring a ULID for every id, and silently overwriting old
results.

## Consequences

ULIDs sort by creation time, so file listings show the queue in order.
Askers that pick their own ids have to keep them unique themselves.
