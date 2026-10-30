# 80. Let the terminal enlarge Kitty images by a divisor of the cell size

Date: 2026-10-30

## Status

Accepted

## Context

- asqr scales every context image to the pixels of the cells it takes
  (ADRs 31 and 32) before it encodes it, also upwards. In Ghostty 1.3.2
  with cells of 15×39 pixels, a 512×512 image in 167×64 cells went out
  at 2505×2496 pixels, 33.50 MB per transmission, and Ghostty had read
  it 117 ms after the write began; through tmux 3.7c, in 164×63 cells,
  32.47 MB took 713 ms.
- A Kitty transmission names the picture's size in pixels (`s`, `v`)
  and the cells of its placement (`c`, `r`) apart, and the terminal fits
  the picture into those cells.
- Sent at its own size, 512 pixels over 65 rows, and enlarged by
  Ghostty, a picture showed seams at cell boundaries. Sent with a whole
  number of its pixels per cell, it showed none.
- Sent at a third of its cells' pixels, 5×13 pixels per cell of 15×39,
  the same image looked as asqr's own scaling does. It went out as
  3.76 MB, read after 12 ms; through tmux as 3.65 MB, after 77 ms.
  Images sent that way showed much faster.
- A 1536×1536 image enlarged 1.6 times into 167×64 cells and sent at a
  third of its cells' pixels, 835×832, looked visibly softer than asqr's
  own scaling.

## Decision

- Under the Kitty protocol, asqr scales an image into its cells' pixels
  divided by `d`, and the terminal enlarges it by `d` into the cells.
  `d` is the largest divisor of the greatest common divisor of the
  cell's width and height that is not larger than the image's
  enlargement into its cells, and 1 where there is none. Each cell then
  holds a whole number of the image's pixels, and the divisor never
  sends an image with fewer pixels than it has.
- Where `d` is 1, asqr sends what it sends under the other protocols:
  for an image shrunk into its cells, an image enlarged less than the
  smallest common divisor above 1, and cells whose sides have no common
  divisor above 1.
- The cells an image takes, with `enlarge` and without (ADR 32), and
  the triangle filter stay as they are.
- Sixel and iTerm2 images stay scaled to their cells' pixels:
  ratatui-image's iTerm2 command names the picture's size in pixels
  (`width=…px;height=…px`), and a Sixel image is drawn at the pixels it
  carries. Half blocks are unaffected.

## Consequences

- With cells of 15×39 pixels, `d` is 3 for an image enlarged 3 times or
  more and 1 otherwise; such an image goes out with a ninth of the
  pixels.
- The terminal does the last enlargement, by `d`, with its own filter.
  How that looks in kitty and with cells other than 15×39 is not
  established.
