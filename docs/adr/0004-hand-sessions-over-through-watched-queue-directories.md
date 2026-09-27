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
