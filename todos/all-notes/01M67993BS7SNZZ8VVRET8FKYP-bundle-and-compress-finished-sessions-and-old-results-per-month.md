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
into a zip and kept; deleting stays a deliberate `prune` with a long
age.

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
- Measured 2026-10-31 (lead), archive and outbox of the default queue
  together, 784 files, 5,626,368 bytes as one tar: zip `-9` 1,070,876;
  tar.gz `-9` 833,467; tar.zst `-19` 558,695; tar.xz `-9` 551,516.
  A zip compresses each file alone and so misses what the many similar
  session files share.
- `skills/asqr/SKILL.md:342` tells agents that results stay in the
  outbox until the person prunes.

## Decisions (Jakob, 2026-10-30, in the chat)

- **Compress, keep.** Finished sessions and results older than 30 days
  are compressed automatically and never deleted by that step. Rejected:
  deleting after an age automatically; manual cleanup only.
- **One bundle per queue and calendar month**, a zip, such as
  `archive/2026-10.zip`, going by the ULID in the entry's name. An entry
  that comes due after its month's bundle exists joins that bundle, so a
  month never has two bundles.
- **A zip, compressed with Deflate** (Jakob, 2026-10-31, replacing the
  tar.zst of 2026-10-30). A zip has an index at its end and compresses
  each file on its own, so `asqr history` and `asqr replay` (todo
  01M67A73S1SRPTR8YRZPPC4FSH) read one session without decompressing
  the kept images around it; Finder and `unzip` open it on every
  machine; and Deflate needs no C library. The bundle holds every kind
  of data of its month in one place, sessions, results and kept files.
  The price is a larger bundle: about 1.07 MB against 0.56 MB for
  tar.zst on the measurement above, which at these sizes does not
  matter. zstd as a format gives the best ratio, but its only usable
  encoder is C (`ruzstd` 0.9.1 documents Default, Better and Best but
  panics `not implemented` on them, checked 2026-10-31), it is not part
  of macOS, and its strengths count at gigabytes. Rejected: tar.zst;
  tar.gz (no index, so a replay reads through every image of the month);
  text in a tar.gz and images in a separate store-only zip (one month
  would sit in two places); 7z, zip with LZMA or zstd entries, zstd with
  a trained dictionary, SQLite (each needs C, a tool macOS lacks, or an
  own format).
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
- No dependency that is not Rust where a Rust one serves: the `zip`
  crate with Deflate through `flate2` and its default Rust backend
  `miniz_oxide`, the same `flate2` that compresses the old logs as gzip.
  The plan checks the `zip` crate's licence, version and MSRV.

## Technical notes (lead, 2026-10-30)

- A month's bundle is extended by rewriting it: read it, add the new
  entries, write a new file beside it, rename over the old one. Entries
  already compressed are copied over without recompressing, so this
  costs well under a second, and every bundle stays one well-formed zip.
  Appending in place is the plan's to weigh against a crash in the
  middle.
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
