# 48. Mend Kitty images an overlay is drawn over

Date: 2026-10-16

## Status

Accepted

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amends [45. Keep image handling in a graphics module around an asqr-owned value](0045-keep-image-handling-in-a-graphics-module-around-an-asqr-owned-value.md)

## Context

- ratatui-image 12.0.0-rc.3 draws a Kitty image as cells of the
  placeholder character U+10EEEE. Only the first cell of each image row
  carries the diacritics for its row, its column and the high byte of
  the image's id; the other cells carry none.
- The Kitty spec lets a placeholder without diacritics take them from
  the placeholder on its left that has the same colours. Where there is
  none, "the terminal may apply other heuristics (but it doesn't have
  to)".
- The help, the intro and the session list are drawn over the context
  pane. Where one covers the first cells of image rows, the cells right
  of it have no placeholder on their left. In Ghostty under tmux
  the image was gone across its full width in every row the overlay
  covered. No other terminal with the Kitty protocol was tried.
- ratatui-image puts the image's transmission in front of the first
  image cell, once per encoding. An encoding first drawn while an
  overlay covered that cell, as after a resize or on regaining the
  focus under tmux with `allow-passthrough on`, lost its transmission
  to the overlay, and the image stayed blank until its next encoding.
- Both are fixed in asqr, and nothing is sent upstream.

## Decision

- Drawing a Kitty image says where its placeholders went, its id and,
  on the first draw of an encoding, its transmission. A viewer's `draw`
  returns it, and the context pane hands it to the screen for that
  frame only.
- Once the overlays are drawn, the screen mends the frame: each
  placeholder of the image whose left neighbour is no placeholder of
  the image gets the diacritics for its row, its column and the id's
  high byte.
- A transmission an overlay covered moves to the first cell of the
  image still drawn. Where the overlay covers the whole image, it moves
  to the top left cell of the screen, which keeps the width it had.
- asqr keeps its own copy of the Kitty spec's table of 297 diacritics.
  A test compares it with what ratatui-image draws.

## Consequences

- With the help open over the test image at 200×30, 120×40 and 160×50,
  15 cells get diacritics, 90 bytes of them. Marks on each of the
  image's 1058 cells at 200×30 would take at least 6 KB on every frame.
  Without an overlay, mending changes no cell.
- An image first drawn under an overlay shows beside it at once, and
  whole once the overlay closes.
- Every viewer's `draw` returns the Kitty image it drew, `None` for all
  but the image viewer.
