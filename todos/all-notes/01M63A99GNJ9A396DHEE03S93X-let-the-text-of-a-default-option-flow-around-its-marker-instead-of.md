---
title: "Let the text of a default option flow around its marker instead of leaving a column"
kind: improvement
component: tui
status: needs-discussion
origin: idea
tags: [ux]
---
# Let the text of a default option flow around its marker instead of leaving a column

Raised by Jakob on 2026-10-29 while answering a real session. When the
first line of a default option's text reaches the `◀ default` marker,
the whole text renders 11 columns narrower on all its lines, so a long
description beside the labels leaves an empty column down the right
edge under the marker. Only the first line needs the room; the lines
below could use the full width, which saves lines in a list where every
option has a long description.

## Goal

Keep the marker's room on the first line only (maybe the first two, see
Open questions) and let the rest of the text wrap at the full width,
like text flowing around an inset.

## Where it is today

- `src/tui/render/content.rs`, `leaving_room`: when `reaches_room`
  finds the first line reaching into the last `room` columns, the whole
  field is rendered again through `field_lines` at `width - room`, so
  every line loses the room.
- `option_lines` passes `marker_room()` (the marker's 9 columns plus
  `MARKER_GAP`) only for a default with its description beside the
  label; a label without a description column takes the same route
  (ADR 0076, "Room": "renders 11 columns narrower, on all its lines").

## Proposal

Wrap the text with a narrower width for the first line (or first two)
and the full width for the rest. `field_lines` renders the Markdown at
one width, so this needs either a per-line width from the renderer
(patine) or rendering the first line separately and the remainder at
full width without breaking the Markdown (lists, quotes, code blocks
that start on the first line).

## Open questions

- Room on the first line only, or on the first two lines? Jakob left
  this open.
- Does this change ADR 0076's room rule, and so need an ADR that amends
  it?
- How does a Markdown block that starts on the first line (a list item,
  a quote, a code block) behave when its width changes after one line?
