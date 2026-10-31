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
