# 91. Rotate the log at 1 MiB and keep old logs compressed

Date: 2026-10-30

## Status

Accepted

Amends [9. Crate choices](0009-crate-choices.md):
`flate2` compresses the old logs

Amends [15. Configure through flags and environment and log into the cache directory](0015-configure-through-flags-and-environment-and-log-into-the-cache-directory.md):
the log rotates, and old logs are kept

Amends [83. Set which events reach the log through ASQR_LOG](0083-set-which-events-reach-the-log-through-asqr-log.md):
the log is rotated

## Context

- Every asqr process appends to `asqr.log` in the platform cache
  directory, and nothing shortened or replaced it. One log held 694,062
  bytes in 5,116 lines written over 33 days, about 21 KB a day.
- Several processes write at the same moment: agents call the CLI while
  the TUI runs.
- On Unix a process that holds a file open keeps writing into it after
  the file is renamed.
- A log of 694,062 bytes compresses to 79,225 bytes as gzip at level 9
  through `flate2`, in 17 ms in a release build, and to 60,908 bytes
  with `zstd -19`. The pure-Rust `ruzstd` crate at `Fastest` makes
  126,837 bytes of the same log. Its documentation lists the levels
  `Default`, `Better` and `Best`, and the encoder panics with `not
  implemented` on them.

## Decision

- When a process opens the log and finds `asqr.log` at or above 1 MiB,
  it moves it to `asqr-<YYYYMMDDTHHMMSSZ>.log` in the same directory,
  the time in UTC, and starts a new `asqr.log`. Growth follows use, not
  time, so a size gives every user the same ceiling. 1 MiB is about 50
  days at 21 KB a day and easy to open in an editor. A TUI that runs
  long can grow the file past 1 MiB with its own lines until the next
  process starts. Once another process has moved the log, a TUI that
  has it open keeps writing into the moved `asqr-<timestamp>.log` until
  it exits, and those lines are not in the file `asqr paths` names.
- A later rotation compresses a moved log as gzip into
  `asqr-<timestamp>.log.gz`. Compressed logs are never removed. A plain
  moved log is replaced by its compressed form only after that form has
  been decoded and found equal to it. An existing `.gz` is never
  replaced.
- A process holds a shared `flock` on the `asqr.log` it opened until it
  ends, and after locking it checks that the path still names the file.
  If a rotation moved the file in between, or a compressor holds the
  lock, it opens the path again, three times at most. If every try
  met one of them, the file at the path is used without a lock, as is a
  file that cannot be locked.
- A compressor takes the exclusive lock of a moved log and compresses
  it only when the lock is granted. Any other result of the lock
  attempt leaves the file plain.
- A rotation does not compress the log it moved itself, and it
  compresses a moved log only when that log was last written to at
  least 24 hours before the rotation. Earlier versions of asqr take
  no lock on the log, so the lock does not show a process of such a
  version, and the log it writes into is recent. A moved log that fails
  a condition stays plain until a later rotation.
- The compressed form is written, and the directory synced, before the
  plain file is removed.
- The processes that rotate hold an exclusive lock on
  `asqr.rotate.lock` in the same directory and check the size again
  once they have it. A process that finds the lock taken does not rotate
  and does not wait. A moved log whose name, or the name of its
  compressed form, exists already is not overwritten, and the log stays
  where it is.
- A rotation or compression that fails leaves the files as they are and
  never stops a command.
- `asqr paths` prints the current `asqr.log`.
- Compression uses the `flate2` crate at `Compression::best()`, with
  its default pure-Rust backend `miniz_oxide`. A log rotates about every
  50 days, so the compression ratio does not matter. gzip opens with
  `zless`, `zgrep` and `gunzip` on every Mac and Linux without installing
  anything, and the zstd command-line tool is not part of macOS. The
  only usable zstd encoder is C code, and building asqr needs no C
  compiler.

Rejected: rotation by age, as a busy day still makes a large file;
rotation by size and age, as it takes two numbers to explain; limits of
5 MiB and 256 KiB; keeping the newest 10 or 3 old logs and dropping the
older ones; keeping one uncompressed old log; the `zstd` crate at level
19; the pure-Rust `ruzstd` crate at `Fastest`, as its output is larger
than gzip's.

## Consequences

A log written by an earlier version may hold text the person typed (ADR
88). Rotation keeps it like any other log.

Compression runs in the command that rotates, before the command goes
on, for each plain log that qualifies. gzip at level 9 takes 17 ms for a
log of 694,062 bytes.
