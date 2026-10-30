---
title: "asqr.log grows without bound"
kind: improvement
component: cli
origin: discussion
---
# asqr.log grows without bound

Every asqr process appends to `asqr.log` in the platform cache directory
and nothing ever shortens or replaces it. On the maintainer's machine it
held 694,062 bytes, 5,116 lines from 2026-09-27 to 2026-10-30, about
21 KB per day. ADR 15 calls logs disposable, as a cache directory
implies, but asqr itself never disposes of anything, so the file grows
until the person deletes it or the system clears its caches. Decided on
2026-10-30: the log rotates at 1 MiB, and old logs are compressed and
kept.

## Problem

`src/cli/log.rs`, `try_init`, opens the file with `create(true)` and
`append(true)` and hands it to `tracing_subscriber::fmt()`. Every
command calls `log::init` (`src/cli/mod.rs:293`); `ask`, `prune`,
`withdraw` and the TUI write lines, `wait` and `result` only open the
file. Several processes can write at the same moment: agents call the
CLI while the TUI runs.

A second finding (2026-10-30): the `replaced a session` line
(`src/cli/ask.rs:178`, `tracing::info!(id, old, ?withdrawn, ...)`)
logs the whole withdrawn `SessionResult` with `Debug`, including the
person's answers and draft text. A line from 2026-10-30 carries
`custom: Some("half")`. So what the person types ends up in the log.

## Decisions (Jakob, 2026-10-30, in the chat)

- **Rotate by size.** When a process opens the log and finds it at or
  above the limit, it moves it aside and starts a new file. Growth
  follows use, not time, so a size gives the same ceiling to everyone.
  Rejected: by age (a busy day still makes a large file), both (two
  numbers to explain).
- **Limit 1 MiB** per file, about two months at today's rate and still
  easy to open in an editor. Rejected: 5 MiB, 256 KiB.
- **Old logs are compressed with zstd and never removed.** Today's log
  compresses to 75 KB with zstd at level 1 (gzip gives 84 KB), so each
  rotation leaves about 110 KB, about 1 MB a year at today's rate.
  Rejected: keep 10 or 3 and drop the oldest; keep one uncompressed old
  file; gzip.
- **Pure Rust over the smallest file.** Compression uses `ruzstd` at
  its only implemented level, `Fastest` (about zstd level 1). Level 19
  would make the same log 61 KB instead of 75 KB at about 150 ms per
  rotation, but needs the `zstd` crate, which builds the C library into
  every release target. Rejected: the `zstd` crate at level 19. asqr
  takes no dependency that is not Rust where a Rust one serves (Jakob).
  Other pure-Rust zstd crates exist, all far younger (crates.io,
  2026-10-30): `zrip` 0.8 (first release 2026-06, levels up to 4),
  `structured-zstd` 0.2 (2026-03, all levels), `oxiarc-zstd`,
  `zstd-rs`, `zstd-pure-rs`. `ruzstd` has 75 million downloads and 295
  dependents, among them `object`, `backtrace` and `cargo-llvm-cov`,
  though they use mostly its decoder; its encoder is the younger part.
- **The log holds no text the person typed.** The `replaced a session`
  line, and any other line that logs a session or its result, logs ids,
  status and counts (such as how many answers), not answers, notes,
  custom text or draft content. Rejected: keep the full dump; a
  separate todo.

## Technical notes (lead, 2026-10-30)

- Hand-written rotation in `src/cli/log.rs` at open:
  `std::fs::metadata`, then rename. `tracing-appender` 0.2.5 rotates by
  time only (checked on docs.rs), so it cannot enforce a size.
  `ruzstd` (MIT, MSRV 1.87, checked 2026-10-30 at 0.9.1) is added with
  `cargo add`.
- Old files carry a timestamp in their name rather than a shifting
  number, since none is ever removed and shifting a growing list on
  every rotation costs a rename per file, for example
  `asqr-20261030T120348Z.log.zst` beside `asqr.log`. The plan decides
  the exact name.
- On Unix a process that still holds the old file open keeps writing
  into it after the rename. Compressing right away would lose those
  lines, so the plan has to say when the moved file is compressed (for
  example, the moved file stays uncompressed until the next rotation
  compresses it, or compression waits until no asqr process holds it).
- Two processes starting at once can both decide to rotate, so the
  rename has to tolerate a missing source (`NotFound`).
- Rotation at open means a long TUI run can grow the file past 1 MiB by
  its own lines until the next process starts. That is accepted.
- Tests point the cache directory at a sandbox (`HOME` and
  `XDG_CACHE_HOME` in `tests/common/mod.rs`). The scratchpad paths in
  the maintainer's log come from agents' manual runs with `--dir`, not
  from the test suite.
- The rule goes into spec section 5.2 (the log paragraph) and a new ADR
  that amends ADR 15, like ADR 83 did; `asqr paths` keeps printing the
  current file.
