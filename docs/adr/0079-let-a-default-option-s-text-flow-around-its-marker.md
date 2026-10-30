# 79. Let a default option's text flow around its marker

Date: 2026-10-30

## Status

Accepted

Amends [76. Mark the default option](0076-mark-the-default-option.md):
only the first line of the text leaves room for the marker

## Context

- `◀ default` stands at the right end of a default option's first line
  (ADR 76). The text that ends that line, the description where
  descriptions stand beside the labels, else the label, rendered 11
  columns narrower on all its lines when its first line would reach the
  marker and its gap.
- Only the first line shares its row with the marker, so a long text
  beside the labels left an empty column under the marker down the
  right edge.

## Decision

- **Room on the first line only:** where the first line of that text
  would reach the marker and its gap, only that line renders 11 columns
  narrower, and the lines below wrap at the full width. This saves lines
  in a list where every option has a long description. A text whose
  first line stays clear keeps the full width on every line.
- **Markdown on the first line:** a paragraph, a heading, a list item or
  a quote wraps its first line narrower and its later lines at the full
  width, a list item's later lines under its text and a quote's behind
  its bar. A table that starts the text is laid out once, for the
  narrower width, on all its rows. A line patine leaves wider than its
  room, such as a line of a code block, wraps on the first row at the
  narrower width and on later rows at the full width, behind `↪`.
- **patine** lays out the narrower first line, through
  `RenderOptions::with_first_line_width` (patine 1.9.0).
- The narrow-pane rules of ADR 76 stay and concern the first line: a
  label keeps at least 20 columns beside its number after the room, and
  descriptions stand beside the labels only while the default's keeps 30
  columns beside the marker.

## Consequences

- The first line of such a text is 11 columns shorter than the lines
  below it.
- asqr needs patine 1.9.0.
