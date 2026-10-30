---
title: "asqr.log grows without bound"
kind: improvement
component: cli
status: needs-discussion
origin: discussion
---
# asqr.log grows without bound

Every asqr process appends to `asqr.log` in the platform cache directory
and nothing ever shortens or replaces it. On the maintainer's machine it
held 684,812 bytes on 2026-10-30. ADR 15 calls logs disposable, as a
cache directory implies, but asqr itself never disposes of anything, so
the file grows until the person deletes it or the system clears its
caches.

## Problem

`src/cli/log.rs`, `try_init`, opens the file with `create(true)` and
`append(true)` and hands it to `tracing_subscriber::fmt()`. Each command
(`ask`, `wait`, `result`, `withdraw`, `prune`) and each TUI run appends
its events. Several of them can write at the same moment: agents call the
CLI while the TUI runs.

## Open questions (the user's to decide)

Found on 2026-10-30 while deciding the level of the Kitty frame timing
line; filed at the user's request:

- Rotate by size or by age?
- How large may the file grow, or how old may lines get?
- How many old files stay (`asqr.log.1`, ...), or none?

## Technical notes (lead, 2026-10-30)

- `tracing-appender` rotates by time interval and names files with a
  date suffix; it has no size limit (to verify against its current
  release before choosing it).
- A size check when a process opens the log, renaming `asqr.log` to
  `asqr.log.1` past the limit, needs no new dependency. On Unix a process
  that still holds the old file open keeps writing into the renamed file,
  so concurrent writers lose no line; two processes starting at once can
  both decide to rotate, so the rename has to tolerate a missing source.
- Whatever rule is chosen goes into spec section 5 (where the log is
  described), and a new ADR amending ADR 15.
