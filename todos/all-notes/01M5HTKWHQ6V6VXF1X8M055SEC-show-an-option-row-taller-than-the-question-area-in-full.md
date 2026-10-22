---
title: "Show an option row taller than the question area in full"
kind: bug
component: tui
impact: low
origin: review
tags: [ux]
---
# Show an option row taller than the question area in full

Found on 2026-10-22 in the plan review for wrapping option labels. An
option row (its label and description) taller than the question's area
shows from its start, and the rest sits under `↓ more` with no key that
reaches it (`src/tui/render/content.rs`, around lines 291 to 308; spec
section 7.4). The tail of such a row shows only once the cursor moves to
a later row; a row taller than about twice the area never shows its
middle lines, and on the question's last row (an option with
`note: false` and no own answer after it) the tail never shows at all.
Long descriptions behave the same.

## Problem

Read in the code and in the review, not reproduced in a test yet
(2026-10-22). It takes a small terminal: at 60×15 an option row gets
about 6 rows without a context pane and about 4 with one below.

## Impact

Text the person decides on stays out of reach on a small terminal.

## Suggested fix

Let a row taller than the area scroll within itself, or move the view so
the rest of the row comes into sight; open which.
