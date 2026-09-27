# 5. Keep all queues under one platform data root

Date: 2026-09-27

## Status

Accepted

## Context

Queues need a default location that works on every platform and that
agents can find without knowing the platform rules.

## Decision

The root is the platform data directory, resolved with the `directories`
crate: `~/Library/Application Support/asqr/` on macOS and
`$XDG_DATA_HOME/asqr/` on Linux. Queues live in `queues/<name>/`, and
the default queue is `default`. All four queue directories, drafts
included, sit under this one root rather than splitting drafts into a
state directory.

The location can be changed with `--queue`, `--dir`, `ASQR_QUEUE` and
`ASQR_DIR`. `asqr paths [--json]` prints the resolved directories.

Rejected: a fixed XDG path on all platforms, and `~/.asqr`.

## Consequences

On macOS the path contains a space, so docs and the skill point askers
to `asqr paths` instead of hard-coding the path.
