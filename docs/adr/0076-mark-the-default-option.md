# 76. Mark the default option

Date: 2026-10-28

## Status

Accepted

Amends [37. Colour the TUI by roles](0037-colour-the-tui-by-roles.md):
the marker of a default option is bold in the primary colour

Amends [55. Wrap option labels and render them as Markdown](0055-wrap-option-labels-and-render-them-as-markdown.md):
the first line of a default option leaves room for its marker

Amended by [79. Let a default option's text flow around its marker](0079-let-a-default-option-s-text-flow-around-its-marker.md):
only the first line of the text leaves room for the marker

## Context

- An option with `default: true` is preselected and drawn like an option
  the person picked, `(•)`, so the person cannot tell the preselection
  from their own choice.
- The agent skill has agents put their recommendation into `default`.
- Option labels and descriptions are full Markdown (ADRs 35 and 55).

## Decision

- **One field:** `default` is the asker's recommendation and the value
  used when the person leaves the question. There is no separate field
  for a recommendation.
- **The marker:** `◀ default`, right-aligned on the first line of every
  option with `default: true`, bold in the primary colour. It stands
  outside the label and the description, so nothing Markdown produces
  looks like it.
- **Always shown:** the marker stays when the person picks another
  option, clears the pick or writes a note. The recommendation stays
  visible, the label never rewraps, and the mark `(•)` and the review's
  `(default)` carry the state.
- **Room:** the marker keeps at least 2 columns between itself and the
  text before it. The text that ends the option's first row, the
  description where descriptions stand beside the labels, else the label,
  renders 11 columns narrower, on all its lines, when its first line
  would reach into the marker and its gap. A text whose first line stays
  clear keeps the full width on every line.
- **Narrow panes:** a label whose first line would come within the
  marker's room keeps at least 20 columns beside its number after that
  room; with less, the whole label goes on the lines below the number and
  the marker stands alone on the option's row. A pane narrower than the
  number, the gap and the marker together, 19 columns or 20 from option
  10 on, shows no marker. Descriptions stand beside the labels only while
  the default's description keeps 30 columns beside the marker.
- **The focus fill:** on the row under the cursor the marker takes the
  fill's one colour like the rest of the row, and so do the blanks
  between the label and the marker. Where a description stands beside,
  which the fill does not cover, only the marker takes it.
- **A cut marker:** `↑ more` or `↓ more` on a line that ends in the
  marker covers the whole marker.
- **The review** keeps `(default)` after an untouched default.

## Consequences

- A default option's label whose first line comes within 11 columns of
  the edge takes all its lines 11 columns narrower than the other
  labels.
- The marker and a chosen mark are both bold in the primary colour. Text
  and place tell them apart: the mark stands before the option's number,
  the marker reads `◀ default` at the right end of the row.
- Under `NO_COLOR` the marker shows bold without colour.
