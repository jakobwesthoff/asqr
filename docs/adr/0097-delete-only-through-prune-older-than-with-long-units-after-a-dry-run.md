# 97. Delete only through prune --older-than with long units, after a dry run

Date: 2026-10-31

## Status

Accepted

Amends [4. Hand sessions over through watched queue directories](0004-hand-sessions-over-through-watched-queue-directories.md):
`asqr prune` no longer takes `--results`, takes other units and deletes
only after `--yes`

Amends [85. Pair asqr and its agents through the tmux session](0085-pair-asqr-and-its-agents-through-the-tmux-session.md):
the removal of named queues that hold nothing is a dry run unless `--yes`

## Context

- `asqr prune --older-than <age>` took the units `s`, `m`, `h`, `d` and
  `w`, so `--older-than 12m` meant 12 minutes and deleted nearly
  everything. `--results` deleted the files of the outbox by their
  modification time, unread results included.
- Sessions and results older than 30 days are compressed into one zip per
  queue and month and kept (ADR 96); deleting is the one step that loses
  data.

## Decision

- Deleting happens only through `asqr prune --older-than <age>`. The age
  takes the units `d`, `w`, `mo` and `y` and is at least 30 days. A month
  is 30 days and a year 365.
- A month bundle is deleted once all of its month is older than the age.
  A loose archive entry is judged by the month of its ULID in the same
  way, so what goes does not depend on whether the compaction has run.
  Results in the outbox are never deleted. The deletion takes the drafts
  lock, under which the compaction renames bundles.
- Without `--yes`, `prune` lists what it would delete and says to run it
  again with `--yes`; with `--yes` it deletes. It never prompts, so it
  behaves the same in a script. The removal of named queues that hold
  nothing follows the same rule.
- `--results` and the units `s`, `m` and `h` are usage errors that name
  the new form. asqr is 0.x, and deleting recent, possibly unread results
  contradicts deletion as a deliberate act.
- `asqr status --since` keeps its own parser and the units `s`, `m`, `h`,
  `d` and `w`.

Rejected: `--before <month>`, and both forms side by side; deleting
directly with an optional `--dry-run`; a prompt on a terminal; deprecating
`--results` and the short units for one release; keeping them.
