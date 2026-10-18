# 12. Use ULIDs as session ids

Date: 2026-10-14

## Status

Accepted

## Context

Session ids name the result files and must be unique per queue. Askers
should not have to invent ids, and a reused id must not destroy an
answer nobody has read yet.

## Decision

- Ids asqr creates are ULIDs: the session ids `asqr ask` assigns, and
  the suffix that keeps archive entries of a reused id apart
  (`archive/<id>.<ulid>.json`).
- The file stem is the session id. The session `id` field is optional;
  an `id` that differs from the stem is a validation error.
- Without an id, `asqr ask` assigns a fresh ULID, writes it into the file,
  names the file after it and prints it.
- `asqr new` prints a session skeleton without an `id`, as the skill
  tells agents to leave it out, and `asqr ask` assigns one.
- A custom id in the allowed syntax (letters, digits, `-`, `_`, `.`) is
  accepted: no leading `.`, at most 200 bytes.
- Ids are compared case-insensitively on every platform.
- An id whose earlier session was answered, with its result still in the
  outbox, is rejected. `asqr ask --force` moves the old result into the
  archive and accepts the new session.
- Waiting sessions are ordered by file modification time, not by id,
  since custom ids do not sort by time.

Rejected: requiring a ULID for every id, and silently overwriting old
results.

## Consequences

ULIDs sort by creation time, so file listings show the queue in order.
Askers that pick their own ids have to keep them unique themselves.
