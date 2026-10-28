---
title: "Let a question text taller than its area scroll into view"
kind: bug
component: tui
origin: discussion
tags: [ux]
---
# Let a question text taller than its area scroll into view

A question's text and its rows (options, a typed field, an info item's
`▸ next` row) share one area that scrolls only far enough to keep the
cursor's row in view. When the text is taller than the room above the
first row, its start sits behind `↑ more` and no key brings it back.
Found on 2026-10-28 while planning the info item: at 80x24 with a context
entry below, an info item gets about four lines of text.

## Problem

`draw_tab` in `src/tui/render/content.rs` computes the scroll from the
cursor's row only. Text above the first row is reachable only while it
fits. Read in the code and in the info item's plan review, not
reproduced in a test yet (2026-10-28).

## Impact

Text the person decides on stays out of reach on a small terminal, for
every kind of question.

## Decided

The user decided on 2026-10-28 that the question area scrolls so every
line of it can be reached, for every question: while the cursor is on
the first row and the text above it does not fully show, `↑` scrolls
the area up line by line through the text until its start shows, and
`↓` scrolls back before the cursor moves on. It uses the same mechanism
as scrolling through an option row taller than the area.

## Relations

- Relates to: [Show an option row taller than the question area in full](01M5HTKWHQ6V6VXF1X8M055SEC-show-an-option-row-taller-than-the-question-area-in-full.md)
