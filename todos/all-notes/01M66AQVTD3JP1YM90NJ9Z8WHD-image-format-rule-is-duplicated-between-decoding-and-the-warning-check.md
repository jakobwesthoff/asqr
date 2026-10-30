---
title: "Image format rule is duplicated between decoding and the warning check"
kind: refactor
component: tui
impact: low
origin: review
---
# Image format rule is duplicated between decoding and the warning check

Two functions decide an image file's format by the same rule, each with
its own copy of the code: the format comes from the file's first bytes,
else from the path's extension, and an extension no format knows fails
with an `Unsupported` error that names it. If one copy changes and the
other does not, `asqr ask` can accept an image the viewer then fails to
show, or warn about one it would show. Found while reviewing the Kitty
scaling work, 2026-10-30.

## Problem

- `src/graphics/mod.rs`, `Images::decode`: reads the format for the
  viewer, from an in-memory `Cursor` over the bytes.
- `src/format/warnings.rs`, `image_header`: reads the format for the
  warning check, from a `BufReader` over the file, then only the
  dimensions.

Both run `with_guessed_format`, then `ImageFormat::from_extension` on a
non-empty extension, and build the same `ImageError::Unsupported`. Their
doc comments say each follows the other ("as `ImageReader::open` finds
it", "as the viewer decides it"); no test checks that the two agree.

## Suggested fix

One function, generic over `BufRead + Seek`, that takes the
`ImageReader` and the path and returns the reader with its format set or
the error. Both callers use it.

The viewer's side has TGA tests (`src/graphics/image.rs` and
`src/tui/render/viewer.rs`, each with a `dot.tga`); the warning check
has none for TGA or for an unknown extension, and neither side tests an
unknown extension. Add those first, so they pin both callers before and
after the change.
