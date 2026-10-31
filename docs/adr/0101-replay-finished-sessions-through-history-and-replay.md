# 101. Replay finished sessions through asqr history and asqr replay

Date: 2026-10-31

## Status

Accepted

## Context

- Archived sessions record the files they showed and keep copies of them
  (ADR 100), so a finished session can be shown again as the person saw
  it.
- Session ids repeat over time and across queues, and agents often choose
  them. The ULID in the archive name of an entry is unique.
- `asqr status` lists answered sessions by id and status only, and reads
  the results, not the archived sessions.

## Decision

### Two commands

`asqr history` prints the list of finished sessions and `asqr replay <ref>`
opens one entry in the terminal UI. `history` is plain output like
`status` and `queues`, while `replay` takes the terminal over, which a
verb says. A bare `asqr history` is the list.

### The list

- `asqr history` covers the queue every command resolves (`--queue`,
  `--dir`, `ASQR_QUEUE`, `ASQR_DIR`, tmux, default). `--all` lists every
  queue under the data directory, one block per queue, the queue in use
  first.
- `--limit` (default 10) and `--since` apply per queue and read like
  those of `status`.
- Columns: REF, WHEN, STATUS, FILES, TITLE. A withdrawn session whose
  result carries a draft shows `withdrawn*` and a footnote.
- `--json` prints the same list for scripts.
- The exit code is 0, 1 or 2. A result status shows in the row, never as
  10 to 14, which are the statuses of the waiting commands.
- A bundle that cannot be read is a warning on stderr and the other
  sessions are listed, as `status` does.

### The reference

An entry is named by the last 8 characters of its archive ULID, which are
random bits, so two entries finished in the same second still differ.
Eight characters are 40 bits and six are 30, so two entries can end in the
same six characters; the commands that take a reference refuse an
ambiguous one and list the candidates. A reference is searched by the
file and bundle entry names alone, so resolving one reads no content.
The session id is not the address.

### Pairing a session with its result

A result answers a session when its `session_sha256` equals the hash the
archived session records in `archived.session_sha256` or the hash of the
file's own bytes. Candidates come from the outbox, from the results `ask
--force` moved aside and from the bundles. Sessions of one id with
identical bytes are told apart by time: the oldest session takes the
latest matching result finished at or before it was archived, and each
result answers one session.
