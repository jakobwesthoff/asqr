# 72. Keep two lines of overlap when pgup and pgdn scroll a page

Date: 2026-10-28

## Status

Accepted

Amends [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md):
`pgup` and `pgdn` scroll the intro overlay by a page less 2 lines

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md):
`pgup` and `pgdn` scroll the pane and the full-screen view by a page
less 2 lines

Amends [52. Scroll half a page with ctrl-u and ctrl-d](0052-scroll-half-a-page-with-ctrl-u-and-ctrl-d.md):
replaces the *Page keys* decision

## Context

`pgup` and `pgdn` scrolled the context pane, the full-screen view and
the intro overlay by the view's height (ADR 30, ADR 31, ADR 52). The
last line shown before `pgdn` was gone after it, so the reader had no
line to hold on to and had to find the place again. Vim's `ctrl-f`
scrolls by the window's height less two lines.

## Decision

- **Overlap:** `pgup` and `pgdn` move by the view's height less 2
  lines, so the last 2 lines before the jump show again at the other
  end, as with Vim's `ctrl-f`.
- **Few rows:** the step is at least 1 line, so in a view of 3 rows or
  fewer `pgdn` moves by 1 line and always moves on.
- **Half a page:** `ctrl-u` and `ctrl-d` keep moving by half the view's
  height (ADR 52).
- **Names:** a page stays the view's height. The help and the key bar
  keep `scroll by page` and `page`; the spec names the step as a page
  less 2 lines.
- In ADR 52 this replaces *Page keys*. In ADR 31 the page step of the
  pane and the full-screen view, and in ADR 30 that of the intro
  overlay, is the height less 2 lines. Where the keys work stays as it
  was.

## Consequences

- In a view of 10 rows, `pgdn` moves 8 lines and two `ctrl-d` move 10.
