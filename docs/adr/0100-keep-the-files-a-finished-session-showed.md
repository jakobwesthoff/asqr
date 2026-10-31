# 100. Keep the files a finished session showed

Date: 2026-10-31

## Status

Accepted

Amends [9. Crate choices](0009-crate-choices.md):
`libwebp-sys` encodes the images asqr keeps, and builds libwebp from C

## Context

- A session shows files by path: images, Markdown, code, ANSI text and
  diff sides. The archive kept the session but not those files, and many
  of them lie in temporary folders. Of 541 file references in 267
  archived sessions of the default queue, 229 (42%) were gone four weeks
  after the oldest session.
- The 245 files still there held 182 MB. 120 of them were images (73 PNG,
  47 JPEG) of 181 MB. On 127 session images, WebP at quality 90 came to
  about 19% of the originals' size.
- No pure-Rust lossy WebP encoder has a licence asqr can use: `image-webp`,
  which `image` builds in, encodes lossless only, and `zenwebp` is
  AGPL-3.0 or commercial. Lossless WebP alone came to 57% of the
  originals.

## Decision

### Modes

`KeepFiles` has three modes:

- `compact` (the default): images are kept as WebP copies, every other
  file as it is.
- `exact`: every file is kept byte for byte, images included.
- `off`: no file is kept. The size and SHA-256 of each shown file are
  still recorded.

### Processors

Before a shown file is kept, a `Processor` chosen by the kind of the entry
that showed it (image, code, Markdown, ANSI, diff side) and the mode turns
its bytes into what is kept. Under `compact` an image goes to `WebpImages`;
every other kind, and every kind under `exact`, goes to `AsIs`. The
encoder sits behind the `ImageEncoder` trait, so another encoder or format
replaces one type. Rejected: the WebP call inlined in the archiving code.

### Re-encoding an image

- The format is read from the file's content, never its name. A WebP, a
  GIF and an animated PNG stay as they are, as does an image that is 0
  pixels or more than 16383 pixels wide or tall (libwebp's limit), and
  one whose decode needs more than 512 MiB.
- The encoder runs at quality 90. The result is decoded back and kept
  only if it has the original's dimensions, is smaller, and both the
  colour and the alpha channel have a PSNR above 30 dB against the
  original. Colour is compared after both pictures are laid over mid-grey
  (128), so the colour libwebp changes under a fully transparent pixel
  does not count. Alpha is compared on its own. Otherwise the original is
  kept.
- The wrapper returns libwebp's failure, such as a picture above its size
  limit, as an `Err`.

### Names

A kept file is named with the lowercased extension of its original name
when it is one to eight characters of `a-z0-9` and an image format
`image` knows, a syntax `two-face` highlights, or one of `txt md log diff
patch ansi` has it. Any other name gets `dat`. A re-encoded image gets
`webp`.

A kept file is named `<sha256>.<ext>` after the SHA-256 of its kept
bytes, so `shasum -a 256` checks it on any machine and a file shown by
several sessions is stored once per place. asqr already uses SHA-256 for
`session_sha256` in results. Rejected: BLAKE3 for the files, with or
without moving `session_sha256` to it. A reader accepts only names of the
form `files/<64 lowercase hex>.<1 to 8 of a-z0-9>`, so a path edited by
hand is never opened.

### Recording

The archived session is the JSON the person saw, edited as raw JSON so
unknown fields survive. It gains an `archived` object (the SHA-256 of the
session as it was in the inbox, which is what a result's `session_sha256`
names, and the mode) and a `shown` object on every entry or diff side
that names a file by path. `shown` holds the original path, a `state`
(`kept`, `not_kept`, `missing` or `unreadable`), the SHA-256, size and
modification time of the original, and for a kept file the size of the
copy. `path` points at the copy only where `state` is `kept`. The
fingerprint is recorded whether a copy is kept or not, so an archived
answer can still tell that a file is gone or changed. A session that
shows no file by path is archived as it is. The format is in spec
section 5.1.

Files are read the way the viewers read them: through
`regular_file::open`, which never waits for a FIFO or a device, and within
the viewers' limits (64 MiB for an image, 8 MiB for any other file). A
file over its limit, a missing file, a relative path or a file that
changes while it is read is recorded with its state and a reason, not
treated as an error. A snapshot holds at most 256 MiB of file content in
memory; a file that does not fit is left to be read later by its path.

`asqr ask` removes `archived` and every `shown` from a session before it
enters the inbox, as asqr alone writes them.

### libwebp

libwebp is C. `libwebp-sys` 0.14.4 builds libwebp 1.6.0 from the C
source that ships in the crate, with the `cc` crate, so a source build
needs a C compiler (the README says so). Its pre-generated bindings are
used, so building needs no libclang. asqr calls the encoder
(`WebPEncodeRGBA`) through a small wrapper, which copies the output into a
`Vec` and frees it with `WebPFree`; images are decoded through `image`.
The `webp` crate was not used: it pins `libwebp-sys` 0.9, which builds
libwebp 1.3.1, older than the release that fixed CVE-2023-4863. `check.yml`
builds both musl targets of the release with `--locked`.

Rejected: JPEG at quality 90 for opaque and lossless WebP for transparent
images (about 25% of the size, pure Rust); AVIF (12%, but `image` decodes
it only through dav1d, C); lossless WebP alone (57%); `libwebpop` 0.1.0,
released 2026-10-25, whose licence statement is unsure.
