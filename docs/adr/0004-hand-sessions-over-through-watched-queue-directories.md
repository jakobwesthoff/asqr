# 4. Hand sessions over through watched queue directories

Date: 2026-09-27

## Status

Accepted

Amended by [63. Keep draft saves and replacements apart with a drafts lock](0063-keep-draft-saves-and-replacements-apart-with-a-drafts-lock.md)

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
- The watcher only considers `inbox/<stem>.json` with a valid id as the
  stem.
- Submit, cancel and error follow one path: the result (with
  `session_sha256`) into the outbox, then the session into the archive
  as `<id>.<ulid>.json`, then the draft is deleted. Each step can be
  repeated safely, and recovery after a crash goes by the hash.
- An invalid session file gets an error result, so an asker never waits
  forever, and is archived after it. An error result never overwrites
  an existing result.
- Replacing a session is a rule of `asqr ask`, not of the queue, since a
  rename replaces an inbox file unconditionally:
  - `ask` places a new session with `persist_noclobber`.
  - It replaces a waiting session only when that session's draft has no
    answer, and refuses otherwise.
  - A file dropped by hand replaces whatever is there.
  - The watcher never writes an error result for a replacement. It
    reloads the file and re-matches the draft.
- Nothing is deleted automatically. `asqr prune --older-than <duration>`
  cleans up on request: archive entries by default, outbox results too
  with `--results`.

Why files: dropping a file already triggers asqr, files queue while asqr
is not running, they survive crashes, and any language can write them
without a client library.

The details are in `docs/spec.md` section 5.

## Consequences

The watcher has to cope with partial writes and editors that save in
several steps. The debouncing and the `.tmp` rule handle that.
Networking and several people on one queue are out of scope.
