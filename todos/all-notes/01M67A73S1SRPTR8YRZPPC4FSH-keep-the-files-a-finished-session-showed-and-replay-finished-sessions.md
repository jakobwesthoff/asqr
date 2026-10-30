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

- **Scope:** one piece of work: `asqr history` (a read-only replay of
  finished sessions in the TUI, with their answers, notes and the kept
  files), keeping the files, and a fingerprint of every shown file
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

## Relations

- Depends on: [Bundle and compress finished sessions and old results per month](01M67993BS7SNZZ8VVRET8FKYP-bundle-and-compress-finished-sessions-and-old-results-per-month.md)
