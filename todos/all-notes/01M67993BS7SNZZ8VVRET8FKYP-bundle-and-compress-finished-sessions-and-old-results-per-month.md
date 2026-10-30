---
title: "Bundle and compress finished sessions and old results per month"
kind: improvement
component: queue
origin: discussion
---
# Bundle and compress finished sessions and old results per month

asqr keeps every finished session and every result forever. Nothing is
removed automatically apart from drafts of sessions that left the inbox
(spec 5.9), and `asqr prune` deletes only when the person runs it. On
the maintainer's machine the data directory held 5.8 MB on 2026-10-30,
four weeks after the oldest archive entry (2026-10-03). Decided on
2026-10-30: entries older than 30 days are bundled per calendar month
into a zstd-compressed tar and kept; deleting stays a deliberate
`prune` with a long age.

## Today (main 313edda)

- Layout (spec 5.1): `inbox/`, `outbox/`, `drafts/`, `archive/` per
  queue. Finished sessions go to `archive/<id>.<ulid>.json`, results to
  `outbox/<id>.json`, which the asker owns.
- Results stay in the outbox until pruned (spec 6.5). asqr cannot tell a
  read result from an unread one.
- `asqr prune --older-than <age> [--results]` (`src/cli/prune.rs`,
  `src/cli/mod.rs:197-206`, spec 5.9) deletes archive entries older than
  the age by the ULID in their name; `--results` also deletes outbox
  files by modification time, unread ones included. Ages take `s`, `m`,
  `h`, `d`, `w`, so `--older-than 12m` is 12 minutes and deletes nearly
  everything. `prune` also removes empty named queues.
- Measured 2026-10-30, default queue: 267 archive entries (3.0 MB on
  disk), 517 results (893 KB of data, 2.3 MB on disk). The archive as one
  tar is 3.2 MB; zstd level 1 makes it 618 KB. One file compressed alone
  shrinks far less, so bundling matters.
- `skills/asqr/SKILL.md:342` tells agents that results stay in the
  outbox until the person prunes.

## Decisions (Jakob, 2026-10-30, in the chat)

- **Compress, keep.** Finished sessions and results older than 30 days
  are compressed automatically and never deleted by that step. Rejected:
  deleting after an age automatically; manual cleanup only.
- **One bundle per queue and calendar month**, a tar compressed with
  zstd, such as `archive/2026-10.tar.zst`, going by the ULID in the
  entry's name. An entry that comes due after its month's bundle exists
  joins that bundle, so a month never has two bundles. The bundle stays a
  standard tar that `tar --zstd -xf` opens.
- **Old results move to the archive.** Results older than 30 days move
  from the outbox into the archive and are bundled with it. An agent
  that comes back late still gets its result. Rejected: leaving the
  outbox alone.
- **When:** the asqr that shows a queue runs the cycle for that queue
  when it starts, for entries older than 30 days. Agents' commands never
  pay for it. Rejected: 7 days; any command.
- **`asqr compact`** runs the same cycle by hand, for a queue no TUI
  opens (a `--dir` queue, a queue no longer used). It never deletes.
  Rejected: `asqr archive`; `asqr prune --compress`, which would make
  prune sometimes delete and sometimes not.
- **Deleting only through `prune --older-than` with long units.** Units
  become `d`, `w`, `mo` and `y`, with a floor of 30 days. A month bundle
  is deleted once all of its month is older than the age. Rejected:
  `--before <month>`; both forms.
- **Dry run unless `--yes`.** Without `--yes`, `prune` lists what it
  would delete and says to run it again with `--yes`; with `--yes` it
  deletes. It never prompts, so it behaves the same in a script.
  Rejected: delete directly with an optional `--dry-run`; a prompt on a
  terminal.
- **`--results` and the units `s`, `m`, `h` go.** Using them is a usage
  error that names the new form. asqr is 0.x, and deleting recent,
  possibly unread results contradicts deletion as a deliberate act.
  Rejected: deprecate for one release; keep them.
- No dependency that is not Rust where a Rust one serves: zstd through
  `ruzstd` (as for the log), tar through the `tar` crate (MIT or
  Apache-2.0, 0.4.46, MSRV 1.63, checked 2026-10-30).

## Technical notes (lead, 2026-10-30)

- A month's bundle is extended by rewriting it: read it, add the new
  entries, write a new file beside it, rename over the old one. A month
  holds a few MB before compression, so this costs well under a second,
  and every bundle stays one well-formed tar.
- `asqr result <id>` and `status --all` or `--since` read the bundles
  too, so a bundled result is still found and listed.
- A result's age comes from its `submitted_at`, else its modification
  time, as `status` orders them (spec 6.5). The plan decides which month
  a moved result's bundle is.
- The cycle needs the queue's lock; the TUI holds it already. `compact`
  takes it without waiting and skips a queue another asqr watches, as
  `prune` does for empty queues. Whether `compact` gets an option for
  every named queue under the root is the plan's to propose.
- The plan defines `mo` and `y` as a number of days.
- Drafts are not touched, as today (spec 5.9).
- Docs: spec 5.1, 5.9 and 6.5, the `prune` help text, the skill line
  above, a new ADR, CHANGELOG.

## Relations

- Relates to: [asqr.log grows without bound](01M66AZ4PM66F0FFF8RFYWNM71-asqr-log-grows-without-bound.md)
