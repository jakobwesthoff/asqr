---
title: "Keep the files a finished session showed, and replay finished sessions"
kind: feature
component: queue
status: blocked
origin: request
tags: [ux]
---
# Keep the files a finished session showed, and replay finished sessions

A session shows files by path: images, Markdown, code, ANSI and diff
sides. The archive keeps the session file but not those files, and many
of them live in temporary folders. An archived answer such as "B" or
"the second mockup" means nothing once the picture is gone. Requested by
Jakob on 2026-10-30 ("with AI sessions, more knowledge and history is
most of the times better than less"): copy the files into the archive
when a session finishes, point the archived session at the copies, and
give the archive a read-only viewer so it can be looked at.

## Measured (default queue, 2026-10-30, sessions since 2026-10-03)

- 267 archived sessions refer to files 541 times: 278 images, 211
  Markdown, 28 code, 13 ANSI. 229 of the 541 (42%) were already gone;
  235 pointed into `/tmp` or agents' scratchpads.
- The 245 files still there hold 182 MB, no two alike: 120 images (73
  PNG, 47 JPEG) of 181 MB, the rest text.
- The 120 images as other formats (cwebp, avifenc and ImageMagick on
  the lead's machine; long side at most 2048 px, which barely changed
  them): lossless WebP 57% of the originals, WebP quality 90 15%, AVIF
  quality 80 12%, JPEG quality 90 23%. Text compresses well with zstd
  (the log: 694 KB to 75 KB).

## Decisions (Jakob, 2026-10-30, in the chat)

- **Scope:** one piece of work: a read-only replay of finished sessions
  in the TUI, with their answers, notes and the kept files, keeping the
  files, and a fingerprint of every shown file
  recorded in the archived session whether files are kept or not. An
  export of a finished session (`asqr export <id>` to Markdown or HTML
  with its images) is a separate, later todo. Rejected: keeping files
  without a viewer; export in the same todo.
- **On by default.** Keeping files is on unless switched off; the
  switch should be easy to find, possibly asked on first start once
  asqr has user settings (todo 01M623QGQXR4ERM34GYV8H6DVR).
- **Copied when the session finishes,** by the asqr that shows the
  queue, as it moves the session to the archive: the files the person
  saw, while agents' temporary files still exist. Rejected: at
  `asqr ask`; at the monthly bundling (42% gone after four weeks).
- **Images as WebP at quality 90 through libwebp**, the original kept
  where it is smaller. libwebp is C: Jakob chose it on purpose as an
  exception to the rule of no dependency that is not Rust, because no
  pure-Rust lossy WebP encoder has a usable licence (`image-webp`, in
  the build, encodes lossless only; `zenwebp` is AGPL-3.0 or
  commercial; checked 2026-10-30). WebP keeps transparency. Rejected:
  JPEG quality 90 for opaque and lossless WebP for transparent images
  (about 25%, pure Rust); AVIF (12%, but `image` decodes it only through
  dav1d, C); lossless WebP alone (57%). Text files are kept as they are
  and compress in the month bundle.
- **Pure-Rust WebP encoders re-checked at pickup.** Research on
  2026-10-30 (conductor logbook,
  `plans/next-todos/webp-encoder-research.md`) found pure-Rust lossy
  encoders, all first released in 2026: `libwebpop` 0.1.0 (BSD-3, a
  libwebp port, output within 0.1% of `cwebp -q 90` on 101 PNGs, but
  released 2026-10-25, LLM-written, and its README unsure of its
  licence), `otf-pixels-codec-webp` 0.5.0 (Apache-2.0, 3% larger opaque,
  13.5% with alpha), and a lossy encoder on `image-webp`'s unreleased
  `main` (basic, an open silent-corruption issue, image-rs/image-webp
  #191). Jakob, 2026-10-30: plan with libwebp; the encoder sits behind a
  small seam, and the agent that picks this up re-checks `libwebpop` and
  an `image-webp` release and switches if one is mature and clearly
  licensed. Rejected for now: `libwebpop` pinned; waiting for
  `image-webp`.
- **`ASQR_KEEP_FILES=compact|exact|off`** until asqr has user settings:
  `compact` (the default) re-encodes images as above, `exact` keeps
  every file byte for byte, `off` keeps no files but still records the
  fingerprints. Rejected: two variables.
- **SHA-256** names the kept files and fingerprints every shown file.
  asqr already uses it (`sha2`, `session_sha256` in results, ADR 28,
  which records no reason for it), and `shasum -a 256` checks it on any
  machine. Rejected: BLAKE3 for the files, with or without moving
  `session_sha256` to it in format version 3.
- **One finished session can be removed on purpose:**
  `asqr prune --session <id>`, a dry run unless `--yes` like every
  delete, rewrites its month's bundle without it and drops kept files
  no other session uses. Part of this todo. Rejected: a separate todo;
  whole months only.

- **The replay is reached through the CLI first** (Jakob, 2026-10-30):
  how a replayed session looks and how one gets to it are two
  questions. In the first version there is no entry point in the TUI: a
  command lists what the archives hold, and a command replays one entry,
  opening the read-only view. The list covers the current queue,
  resolved as every command resolves it, and `--all` covers every queue
  under the data directory (with `--since` and `--limit` as `status`
  has them). Each row shows a short reference unique to the entry,
  taken from the ULID in its archive name, since session ids repeat
  across queues and over time; the replay command finds that reference
  in every queue under the data directory without a queue named (a
  `--dir` queue outside it is named). Rejected for now: a list or key in
  the TUI; a list over every queue by default; replay by session id
  within the current queue.
- **Commands and reference** (Jakob, 2026-10-30, after the design study
  in the conductor logbook, `plans/replay-design/`): `asqr history`
  prints the list, `asqr replay <ref>` opens one entry and takes the
  terminal over, which the verb says. The list's columns are REF, WHEN,
  STATUS, FILES and TITLE. The reference is the last 8 characters of the
  entry's archive ULID, its random part, so entries finished in the same
  second still differ; `replay` accepts any end of it of 6 or more
  characters in any case, and refuses one that is too short, matches
  nothing or ends two entries, listing the candidates. Rejected:
  `archive` and `show` (reads like `asqr compact` and the bundles);
  `history list` and `history show`; the start of the ULID as git does
  (the timestamp, shared by every entry of a day); accepting a unique
  session id as well.
- **The look is direction D of the study** (Jakob, 2026-10-30): the
  session screen with the answers filled in and nothing writable. The
  options the person did not pick are dim, the picked one keeps `(•)`.
  The status line carries `◷ replay · sent <time> · read-only` for the
  whole replay; a notice replaces it for a moment. The title bar names
  the queue and the reference. The cursor still moves over every option
  to show its context. The replay opens on the first question, and the
  last tab is `Result` (how it ended, every answer on a line). A new key
  `d` opens the details of a kept file (copy, original path, size, time,
  SHA-256). A missing file shows asqr's red card with its original path,
  a file not kept a card with size, time and fingerprint, and a withdrawn
  session its draft dimmed and marked `◀ draft`. Rejected: a strip row
  under the intro (A), alone or with the dimming; a bar down the left
  edge (B); opening on the `Result` tab (C).
- **A read-only mark that always shows** (Jakob, 2026-10-30): `◷
  read-only` in the warning colour, without a fill, right after `asqr`
  in the title bar. The title bar shows in every state of a session,
  the full-screen view and overlays included, and its left end is never
  cut, so a notice cannot hide the mark the way it hides the status
  line. The clock says "from the past" rather than "forbidden".
  Rejected: a filled badge there; the mark at the right end before the
  count; a badge in place of `asqr`; `⊘`; the word alone.
- **Arrivals during a replay** (lead, 2026-10-30): `asqr replay` holds
  no queue lock and writes nothing; it shows a count of waiting sessions
  in the title bar and says in the status line where to answer. It
  sends no notification and has no key to hand over to the live TUI in
  the first version.
- **Details left to the lead** (2026-10-30): `--limit` counts per queue
  under `--all`; an entry whose month bundle cannot be read ends the
  replay with an error naming the bundle and a non-zero exit. `--json`
  on `history` is in if cheap; search and the in-TUI history wait.

## Technical notes (lead, 2026-10-30)

- The archived session points at each copy: its `path` names the copy
  inside the archive, and the original path stays beside it in a field
  of its own, so the replay shows the copy and the person sees where it
  came from.
- Copies are stored under their hash, so a file shown by several
  sessions is kept once, and go into the same month bundle as the
  sessions that showed them (todo 01M67993BS7SNZZ8VVRET8FKYP).
- asqr reads a shown file within its usual limits (images up to 64 MiB,
  text up to 8 MiB); a file it cannot read is recorded as missing in the
  archived session, not an error.
- The replay reads the month bundles directly.
- Every re-encoded image is decoded back (`image` decodes WebP); when
  that fails, the original is kept instead.

## Relations

- Depends on: [Bundle and compress finished sessions and old results per month](01M67993BS7SNZZ8VVRET8FKYP-bundle-and-compress-finished-sessions-and-old-results-per-month.md)
