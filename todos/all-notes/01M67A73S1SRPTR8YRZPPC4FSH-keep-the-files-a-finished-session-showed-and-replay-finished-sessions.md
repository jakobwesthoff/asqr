---
title: "Keep the files a finished session showed, and replay finished sessions"
kind: feature
component: queue
status: needs-discussion
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
- **Images are re-encoded to save space,** Jakob's suggestion: WebP at
  a quality around 90 to 95, and an option (a setting, an environment
  variable for now) that keeps the files exactly as given instead.
  Text files are kept as they are and compress in the month bundle.
- **Hash:** Jakob suggested BLAKE3 for the fingerprints and the copies'
  names, and asked for the lead's opinion.

## Open (lead, 2026-10-30, to discuss with Jakob)

- **No pure-Rust lossy WebP encoder under a usable licence.**
  `image-webp` (in asqr's build through `image`, MIT or Apache-2.0)
  encodes lossless WebP only (docs.rs, `WebPEncoder::new`: "Only
  supports VP8L lossless encoding"). `zenwebp` 0.4 encodes lossy WebP in
  pure Rust but is AGPL-3.0 or commercial. The `webp` crate wraps
  libwebp (C), which the rule of no dependency that is not Rust where a
  Rust one serves excludes. Candidates that stay in pure Rust:
  - JPEG quality 90 through `image` (encoder in `image`, decoder
    `zune-jpeg`, both in the build): 23%; no transparency, so an image
    with alpha needs lossless WebP.
  - AVIF through `ravif`/`rav1e`, already in the build through `image`'s
    `avif` feature without the `asm` feature: 12%. But `image` decodes
    AVIF only with `avif-native` (dav1d, C), so the replay could not
    show the copies without a C decoder. Not verified whether a
    pure-Rust AV1 decoder is usable.
  - Lossless WebP through `image-webp`: 57% with cwebp; `image-webp`'s
    own encoder is not measured and is likely larger.
- **SHA-256 or BLAKE3.** asqr already depends on `sha2` and its results
  carry `session_sha256` (spec 4.1); `shasum -a 256` verifies it on any
  machine. BLAKE3 (`blake3` 1.8.7) is faster but needs its `pure`
  feature to avoid its C and assembly code, and `b3sum` to verify by
  hand. At about 180 MB a month, speed does not matter.
- How the archived session points at a copy: the path rewritten to the
  copy with the original path kept beside it, or the session left as
  sent with a map file beside it.
- Whether a single finished session can be removed on purpose (a secret
  in a screenshot), such as `asqr prune --session <id> --yes`, which
  rewrites its month's bundle without it.

## Technical notes (lead, 2026-10-30)

- Copies are stored under their hash, so a file shown by several
  sessions is kept once, and go into the same month bundle as the
  sessions that showed them (todo 01M67993BS7SNZZ8VVRET8FKYP).
- asqr reads a shown file within its usual limits (images up to 64 MiB,
  text up to 8 MiB); a file it cannot read is recorded as missing in the
  archived session, not an error.
- The replay reads the month bundles directly.

## Relations

- Depends on: [Bundle and compress finished sessions and old results per month](01M67993BS7SNZZ8VVRET8FKYP-bundle-and-compress-finished-sessions-and-old-results-per-month.md)
