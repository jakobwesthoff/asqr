# 4. Hand sessions over through watched queue directories

Date: 2026-09-27

## Status

Accepted

## Context

An asker, such as an agent or a script, has to hand questions to an asqr
instance running in another terminal and read the answers back. A socket
was considered.

## Decision

Sessions travel as files only, with no socket. A queue has four
directories: `inbox/` (written by the asker), `outbox/` (results written
by asqr), `drafts/` (answers in progress) and `archive/` (answered
sessions).

- Writers write a `.tmp` file and rename it; asqr ignores `*.tmp`.
- On submit asqr writes the result into the outbox, then moves the
  session into the archive.
- An invalid session file gets an error result, so an asker never waits
  forever.
- Nothing is deleted automatically. `asqr prune --older-than <duration>`
  cleans up the archive on request.

Why files: dropping a file already triggers asqr, files queue while asqr
is not running, they survive crashes, and any language can write them
without a client library.

## Consequences

The watcher has to cope with partial writes and editors that save in
several steps. The debouncing and the `.tmp` rule handle that.
Networking and several people on one queue are out of scope.

## Amendment (2026-09-27, adversarial review F1, F3, F5, F12)

- **Replacing a session is a rule of `asqr ask`, not of the queue**, since
  a rename replaces an inbox file unconditionally.
  - `ask` places a new session with `persist_noclobber`.
  - It replaces a waiting session only when that session's draft has no
    answer, and refuses otherwise.
  - A file dropped by hand replaces whatever is there.
  - The watcher never writes an error result for a replacement. It
    reloads the file and re-matches the draft.
- **The watcher only considers `inbox/<stem>.json` with a valid id as the
  stem.** Temp files use the `.tmp` suffix. Invalid files are archived
  after their error result, and an error result never overwrites an
  existing result.
- **Submit, cancel and error follow one path:** result (with
  `session_sha256`), then archive, then delete the draft, and each step
  can be repeated safely. Recovery after a crash goes by the hash.
  Archive names are `<id>.<ulid>.json`.
- **`prune` removes only archive entries by default.** `--results` also
  removes outbox results.

The details are in `docs/spec-1.0.0.md` section 3.
