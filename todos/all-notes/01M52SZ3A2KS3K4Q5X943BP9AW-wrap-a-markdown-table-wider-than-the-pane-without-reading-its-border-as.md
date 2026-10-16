---
title: "Wrap a Markdown table wider than the pane without reading its border as a quote bar"
kind: bug
component: tui
status: needs-discussion
origin: review
tags: [ux]
---
# Wrap a Markdown table wider than the pane without reading its border as a quote bar

Found in the review of the patine 1.7.1 pickup on 2026-10-16. A
Markdown table that patine renders wider than the context pane breaks
apart in asqr: every row wraps, and the wrapped part of a row starts
with the table's left border, which asqr's wrapping reads as a quote
bar. Since patine 1.7.2 a table breaks a long word inside its cell and
fits, so only rare tables still reach this.

## Problem

Reproduced on 2026-10-16 with patine 1.7.1, in a throwaway test through
the Markdown viewer (`src/tui/render/markdown_viewer.rs`, its `inline`
test helper) at a width of 40 columns. The entry's text:

```
| step | doc |
|--|--|
| retry | see [the policy](https://example.com/handbook/operations/retry-policy) |
```

The first eight rows it drew:

```
┌───────┬───────────────────────────────
↪ ─────────────────────────┐
│ step  │                          doc
│ ↪                          │
├───────┼───────────────────────────────
↪ ─────────────────────────┤
│ retry │ see the policy
│ ↪                          │
```

With patine 1.7.1 a table cell keeps its inline styles and a link
shows its `(url)`, so the long URL set the column's width and the
table rendered 66 columns wide at a width of 40. asqr's adapter
(`src/tui/render/patine_adapter.rs`, `indent`) wraps each line wider
than the pane and repeats the blanks and quote bars it starts with, and
the border `│ ` of a table row counts as such a bar. Until patine 1.7.2
the spec named this wrap as intended for "a table that cannot shrink"
(section 7.7).

Reproduced again on 2026-10-16 with patine 1.7.2, through the same
viewer: the table above fits in 7 rows of 40 columns, its URL broken
inside the cell. Two cases still wrap the way shown above:

- A table whose columns do not fit even at one character each: twelve
  one-letter columns need 49 columns, so at a width of 20 every border
  and row wraps.
- A table row with a halfwidth sound mark such as `ﾞ`. patine counts
  `ｶﾞ` as one column (unicode-width gives the mark none), asqr as two
  (ADR 0033), so a row patine fits to the pane is wider on screen. A
  cell of `ｶﾞ` six times at a width of 12 draws `│ x │ ｶﾞｶﾞｶﾞ` and then
  `│ ↪ ｶﾞ │`.

The test `a_table_row_patine_leaves_wider_still_wraps` in
`src/tui/render/markdown_viewer.rs` covers both cases. The spec (section
7.7) and the amendment "tables break long words" of ADR 0034 name them.

## Impact

A table with more columns than the pane holds at one character each,
or with halfwidth katakana that fills its row, cannot be read in a
narrow context pane or text field.

## Open questions

- How a table that still does not fit should show: wrapped row by row
  without treating the border as a quote bar, cut at the pane's edge
  with sideways scrolling, or something else. This is Jakob's call, and
  it decides the spec 7.7 sentence too.
- Whether the halfwidth sound mark case belongs to patine (measuring a
  line as terminals draw it) or to asqr.

## Decisions

- 2026-10-16, Jakob: patine breaks a word longer than its table column
  across lines, so a table stays whole and fits. That landed in patine
  1.7.2, and asqr picked it up on 2026-10-16 (ADR 0034, amendment
  "tables break long words"). The cases above remain.
