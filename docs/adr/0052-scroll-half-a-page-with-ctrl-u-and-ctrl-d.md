# 52. Scroll half a page with ctrl-u and ctrl-d

Date: 2026-10-22

## Status

Accepted

Amends [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md)

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [58. Name ctrl-u and ctrl-d in the general help and the key bar](0058-name-ctrl-u-and-ctrl-d-in-the-general-help-and-the-key-bar.md):
the general help names how far `ctrl-u` and `ctrl-d` scroll

Amended by [72. Keep two lines of overlap when pgup and pgdn scroll a page](0072-keep-two-lines-of-overlap-when-pgup-and-pgdn-scroll-a-page.md):
`pgup` and `pgdn` move by a page less 2 lines

## Context

`ctrl-u` and `ctrl-d` scrolled the context pane, the full-screen view
and the intro overlay by a page, the view's height, as `pgup` and
`pgdn` do (ADR 30, ADR 31). In Vim and `less`, where the keys come
from, they scroll half a page: Vim by its `scroll` option, half the
window's height rounded down, and `less` by half the screen. Vim
scrolls a whole page with `ctrl-f` and `pgdn`, `less` with `space`, `f`
and `ctrl-f`.

## Decision

- **Half a page:** `ctrl-u` and `ctrl-d` scroll the context pane, the
  full-screen view and the intro overlay by half the view's height,
  rounded down, and at least one line. The keys come from Vim and
  `less`, where they move half a page.
- **Page keys:** `pgup` and `pgdn` keep scrolling by the view's height.
- **No `ctrl-f` and `ctrl-b`:** in a field they move the text cursor by
  a character (spec section 7.7).
- In ADR 31 this changes the *Page keys* decision and the shared scroll
  conventions, where a page is the pane's height: `ctrl-u` and `ctrl-d`
  move half of it. In ADR 30 it changes the intro overlay's keys the
  same way. Where the keys work stays as it was: from anywhere while a
  pane shows, the same on every keyboard layout, and without a pane a
  field keeps them for undo and delete.
- The help of the focused pane and of the full-screen view lists
  `ctrl-u/ctrl-d  scroll by half a page` on a line of its own, below
  `pgup/pgdn  scroll by page`. The general help keeps its 13 lines and
  its line `pgup/pgdn  scroll the context pane, also ctrl-u/ctrl-d`,
  which names no distance.

## Consequences

- On a keyboard without page keys, two `ctrl-d` move a whole page in a
  view of even height, and one line less in one of odd height from
  three rows on.
- The help of the focused pane has 10 lines and that of the
  full-screen view 9.
