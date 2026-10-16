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
bar.

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

Since patine 1.7.1 a table cell keeps its inline styles and a link
shows its `(url)`, so the long URL sets the column's width and the
table renders 66 columns wide at a width of 40. asqr's adapter
(`src/tui/render/patine_adapter.rs`, `indent`) wraps each line wider
than the pane and repeats the blanks and quote bars it starts with, and
the border `│ ` of a table row counts as such a bar.

The spec describes this wrap as intended (section 7.7): "A line patine
leaves wider than the pane, such as a line of a code block, a long URL
or a table that cannot shrink, wraps: each further row repeats the
blanks and quote bars the line starts with, then blanks in place of a
list item's `•`, `☐` or `☑`, or its number with the box after it, then
a dim `↪`, then the rest."

## Impact

A table with a long link, or with more columns than the pane holds at
their narrowest, cannot be read in a narrow context pane.

## Open questions

- How a table that still does not fit should show: wrapped row by row
  without treating the border as a quote bar, cut at the pane's edge
  with sideways scrolling, or something else. This is Jakob's call, and
  it decides the spec 7.7 sentence too.

## Decisions

- 2026-10-16, Jakob: patine breaks a word longer than its table column
  across lines, so a table stays whole and fits; that lands in patine
  1.7.2, and asqr picks it up afterwards. That makes this case rare:
  it stays possible, for example with more columns than the pane can
  hold at their minimum width.
