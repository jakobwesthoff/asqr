---
title: "Find out how GitHub marks the changed words of a diff"
kind: investigation
component: tui
horizon: someday
origin: discussion
tags: [ux, field-test]
---
# Find out how GitHub marks the changed words of a diff

Jakob, 2026-10-16: marking changed words inside a diff did not work in
asqr, neither as built (similar's inline changes over a whole block)
nor in the three variants prepared for him (words across a block with
line breaks and comment leaders as blank, the same without marks on
wholly new lines, and line pairs at 40 % similarity as delta does). He
decided that asqr shows diffs line by line without marked words, and
that it is to be found out how GitHub and others make the marking work.

## What to find out

How GitHub, GitLab, delta and difftastic pair lines and decide which
words to mark, on the cases that failed in asqr: a reflowed comment
block, a reworded Markdown paragraph, a moved line next to changed
ones, re-indented code. Renders of asqr's variants on those cases and
the prototype are in
`~/playground/2026_10_15_asqr-maskottchen/vorbereitung/diff/`.

## Revisit when

No trigger known; reconsider at the next sweep.
