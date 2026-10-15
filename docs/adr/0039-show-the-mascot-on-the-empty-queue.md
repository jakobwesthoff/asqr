# 39. Show the mascot on the empty queue

Date: 2026-10-15

## Status

Accepted

## Context

With no session waiting, the main area held one centred sentence,
"Nothing to answer. New sessions show up here as soon as they arrive.",
and was otherwise empty. On 2026-10-15 the user chose to fill that space
with asqr's mascot, the chameleon Talqi (pronounced like "talkie"), whom
the design guide names. The originals are PNGs of 1536×1536 pixels under
`assets/`, which `Cargo.toml` leaves out of the published crate.

ratatui-image 11.1.0, which draws asqr's images, and image 0.25.10,
which decodes and resizes them, behave as follows where it matters here:

- With `Resize::Fit`, ratatui-image never enlarges a picture, and every
  protocol draws from the area's top-left corner. Nothing centres it.
- ratatui-image resizes a picture before encoding it unless the picture
  fills its cells exactly on at least one axis.
- Both resize each channel on its own, without premultiplying alpha. The
  transparent pixels of the mascot store black, so a plain resize
  darkens the picture's edges.
- ratatui-image's half blocks convert the picture with `to_rgb8()`,
  which drops the alpha channel: the transparent background would show
  as coloured blocks.

## Decision

Which picture (the user, 2026-10-15):

- From asqr's start until the first session arrives, the empty queue
  shows Talqi awake (`assets/mascot-1536.png`). Once sessions came in
  and the queue is empty again, it shows Talqi asleep
  (`assets/mascot-variants/mascot-sleeping-no-moon.png`).
- This holds per run of asqr. Nothing is stored on disk, so every start
  begins with Talqi awake.

How the pictures are built in:

- Both are part of the executable as lossy WebP of 768×768 pixels,
  quality 90 with full alpha quality, under `src/tui/render/mascot/`
  (74 KB awake, 71 KB asleep). They are made with `cwebp`, as the user
  asked:

      cwebp -q 90 -alpha_q 100 -m 6 -metadata none -resize 768 768 \
          <original>.png -o <copy>.webp

- The size is 768 pixels because 44 columns of cells 15 pixels wide,
  as the user's Ghostty reports them, take 660 pixels, more than a
  picture of 512 pixels holds. 512 pixels was the first choice.

Size and place:

- The picture is drawn at about the size of a half-block rendering, as
  the user chose: at most 44 columns by 22 rows, the cells a square
  picture takes in half blocks of one pixel wide and two tall. Where
  the area is smaller, the picture is smaller. It is never stretched to
  fill the area.
- The side of the square in pixels is the smaller of that box's width
  and height in pixels, cells times the cell size asqr learned from the
  terminal at start. The picture's cells are that side divided by the
  cell size, rounded up on each axis.
- The picture shows only when the main area has at least 12 rows and
  30 columns (the user, 2026-10-15). It gets the main area less the 3
  rows of the sentence and one blank row. Picture, blank row and
  sentence form one group, centred down the main area, with the picture
  centred across, so it stands directly above the sentence.

Drawing:

- asqr scales the picture to the side in pixels itself, with Lanczos3,
  each colour multiplied by its alpha before and divided by the new
  alpha after. The scaled picture fills its cells exactly on one axis,
  so ratatui-image's `Resize::Fit` encodes it as it is.
- The picture is drawn through the same image type as the images of
  context entries. It is decoded, scaled and encoded when a picture or
  a size is drawn for the first time, and kept until the next one.
- When asqr's terminal gains the focus, the encoded picture is dropped
  like the images of entries (ADR 24), so the next draw sends it again.
- Nothing clears the screen when the first session replaces the picture.
  ratatui 0.30.2 (ratatui-core 0.1.2) counts a cell's diff option in
  the cell's equality and leaves out only the cells the new frame marks
  as skipped. The cells an image marked as skipped in the last frame are
  therefore written again when the new frame has text or blanks there.
- In a terminal without an image protocol the sentence stands alone.
  The user chose half blocks in Talqi's own colours as the fallback
  there; they are not part of this decision.

## Consequences

- The main area of the empty queue has 6 rows less than the terminal,
  so Talqi shows from 18 rows on. At the smallest size asqr draws,
  60×15, the sentence stands alone.
- Each new size of the picture decodes and scales the 768 pixel picture
  again.
- Picture and sentence take a group of rows that is centred as a
  whole. On a screen wide enough for the sentence on one line, two of
  its three rows stay blank, so the group's visible part sits a little
  above the middle.
