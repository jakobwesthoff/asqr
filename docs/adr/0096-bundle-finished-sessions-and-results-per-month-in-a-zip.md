# 96. Bundle finished sessions and results per month in a zip

Date: 2026-10-31

## Status

Accepted

Amends [9. Crate choices](0009-crate-choices.md):
the `zip` crate reads and writes the month bundles of the archive

## Context

- A queue kept every finished session in `archive/` and every result in
  `outbox/` as a file of its own. Nothing removed them but a `prune` the
  person ran.
- The default queue held 267 archive entries (3.0 MB on disk) and 517
  results (893 KB of data, 2.3 MB on disk) four weeks after its oldest
  entry. Archive and outbox together are 784 files and 5,626,368 bytes as
  one tar; compressed, that is 1,070,876 bytes as a zip at level 9,
  833,467 as tar.gz, 558,695 as tar.zst at level 19 and 551,516 as tar.xz.
  A zip compresses every file on its own and so misses what the many
  similar session files share.
- A tar.zst gives the best ratio, but the only usable zstd encoder is C
  code and zstd is not part of macOS. `ruzstd` 0.9.1 documents the levels
  `Default`, `Better` and `Best` and panics with `not implemented` on
  them.
- `zip` 9.1.3 is MIT licensed and needs Rust 1.88. With
  `default-features = false` and the feature `deflate-flate2` it pulls
  `flate2` (already a dependency, with its pure-Rust backend), `crc32fast`,
  `getrandom`, `indexmap`, `memchr`, `nix` and `typed-path`, and builds
  without a C compiler. Its default features would add `bzip2`, `zstd`,
  `lzma` and `aes`. Ten releases came out between 2026-10-09 and
  2026-10-27.
- `ZipWriter::raw_copy_file` of `zip` 9.1 drops the standard
  extended-timestamp extra field (`0x5455`) when it copies an entry over.
  An extra field with an id of its own survives.

## Decision

- Sessions and results older than 30 days are compressed and kept. Only
  the loose file of an entry that is proven to be inside its bundle is
  deleted; deleting data stays with `asqr prune` (section 5.9 of the
  specification).
- One zip per queue and UTC calendar month, `archive/<YYYY-MM>.zip`,
  going by the ULID in the entry's name. An entry that comes due after
  its month's bundle exists joins that bundle, so a month never has two.
- The bundle is a zip, compressed with Deflate at level 9 through
  `flate2`. A zip has an index at its end and compresses each file on its
  own, so `asqr history` and `asqr replay` can read one session without
  decompressing the kept images around it. Finder and `unzip` open it on
  every machine, and Deflate needs no C library. The bundle holds every
  kind of data of its month in one place: sessions, results and kept
  files. The price is a larger bundle, about 1.07 MB against 0.56 MB for
  tar.zst on the measurement above.
- Entries are `sessions/<id>.<ulid>.json`, `results/<id>.<ulid>.json` and
  `replaced/<id>.<ulid>.json` (a result `ask --force` moved aside). The
  prefix `files/<sha256>.<ext>` is reserved for the files a session
  showed. A later kind of entry only adds names.
- Results older than 30 days move from the outbox into the archive and
  are bundled with it, so an agent that comes back late still gets its
  result. A result lies in the month it was finished in: the ULID of its
  entry is built from its finish time (`submitted_at`, else the file's
  modification time) and the SHA-256 of its bytes, so a retry after an
  interruption finds the entry again.
- Every entry records the time it was bundled in an extra field with the
  id `0xA5C1`: four bytes of Unix seconds in UTC, little endian. The
  newest bundled result of an id answers `result` and `wait`, unless a
  session of that id was archived after the result was bundled: such a
  session has no result. A session archived unanswered next to an unread
  result was archived before the bundling and so still shows the result.
  An entry without the field is never hidden.
- The asqr that shows a queue runs the compaction for that queue when it
  starts or switches to it, so agents' commands never pay for it.
  `asqr compact` runs the same compaction by hand, for a queue no TUI
  opens, such as a `--dir` queue or a queue no longer used, and
  `asqr compact --all` does it for every named queue, skipping a queue
  another asqr watches. It never deletes data.
- A bundle is extended by writing a new file beside it, syncing it,
  reading every added entry back and renaming it over the old one.
  Entries that are compressed already are copied without compressing them
  again. The new file is built without a lock; the drafts lock covers the
  check that the bundle and every loose file are as they were, the rename
  and the removal of the loose files, so a `prune` that deleted a bundle
  in between is noticed and the bundle does not come back. A month where
  the check fails is left for the next run.
- Readers list the loose files before they open a bundle, so an entry
  that is moving is seen twice at worst.
- `asqr result`, `wait`, `status`, `withdraw` and `queues` read the
  bundles through the module `queue::bundle`, which also serves
  `archived_title` for `follows`. `QueueSummary.answered` counts the
  outbox alone, as the TUI asks for it every five seconds; only
  `asqr queues` adds the bundled results.
- Compaction runs in the process before the screen is drawn. On the real
  archive and outbox above, a release build bundled all 784 files into
  two bundles of 146,058 and 926,606 bytes in 253 ms.

Rejected: tar.zst; tar.gz, which has no index, so a replay reads through
every image of the month; text in a tar.gz and images in a separate
store-only zip, as one month would sit in two places; 7z, zip with LZMA or
zstd entries, zstd with a trained dictionary and SQLite, each of which
needs C, a tool macOS lacks or a format of our own; deleting after an age
automatically; manual cleanup only; leaving old results in the outbox;
bundling after 7 days or at any command; `asqr archive` and
`asqr prune --compress` as the name of the command.

## Consequences

- A result older than 30 days is no longer in the outbox, so it no longer
  stops a new `ask` of its id; `asqr result <id>` still prints it from the
  archive until a new session of that id is asked, and then reports that
  session as waiting.
- `zip` is released often (ten releases in 18 days, 9.0.0 to 9.1.3);
  `Cargo.lock` fixes the version.
- The extra field `0xA5C1` carries the bundling time because the standard
  field does not survive `raw_copy_file`.
