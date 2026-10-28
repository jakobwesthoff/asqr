---
title: "The focus fill of an option leaves the indent of wrapped label lines unfilled"
kind: bug
component: tui
status: needs-discussion
impact: low
origin: review
tags: [ux]
---
# The focus fill of an option leaves the indent of wrapped label lines unfilled

Found in a plan review on 2026-10-28. Under the cursor, an option whose
label wraps shows the focus fill on the label's text on every line, but
not on the blanks that indent lines 2 and later. No line of an option
fills the blanks after its text. The documents disagree on whether they
should.

## Problem

`option_lines` in `src/tui/render/content.rs` puts the fill on each span
of the head and of each label row (`theme::focus_span`). Label rows
after the first go through `indented` in `src/tui/render/wrap.rs`, which
puts an unstyled span of blanks before them, and no line gets the fill
as its line style, so nothing after the text is filled. Where a narrow
pane puts the whole label below the option's row, every label line has
the unfilled indent.

The review's question rows use `theme::focus_row`, which sets the line's
style as well, so they fill to the edge of the area.

Read in the code on 2026-10-28, not checked in a terminal.

## What the documents say

- ADR 0037: "a filled row takes the fill's colour in every part".
- Spec section 7.3: the row under the cursor shows the focus fill "in
  one colour over the whole row".
- Spec section 7.2 and ADR 0055: under the cursor, the focus fill
  "covers the pointer, the mark, the number and every line of the
  label".

The first two speak of the whole row. The last two list the filled
parts, and the indent and the blanks after the text are not among them.

## Open questions

- Which document is right: fill an option's lines to the edge as the
  review rows are, fill only the indent of later label lines, or keep
  the code and make ADR 0037 and spec section 7.3 say what an option
  row fills.

## Order

Another change is adding a right-aligned `◀ default` marker to option
rows in `src/tui/render/content.rs`, which takes the focus fill under
the cursor. This fix comes after that change lands.
