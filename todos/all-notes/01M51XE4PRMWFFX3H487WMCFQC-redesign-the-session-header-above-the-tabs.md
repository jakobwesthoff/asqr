---
title: "Redesign the session header above the tabs"
kind: improvement
component: tui
origin: request
tags: [ux, field-test]
---
# Redesign the session header above the tabs

Jakob, 2026-10-16: the title area with its short explanation looks
squeezed, and he hardly ever notices it, only the questions. Today the
header is four lines without space between them: the title line
(`asqr`, queue, waiting count), the session's title with `from` and
`follows`, the intro dimmed in up to three lines, then the tabs above a
rule.

## Decisions (Jakob, 2026-10-16, asqr session 01M51YYPA5SXZ8G7Y7M4PHG1F6)

Chosen from seven rendered variants (renders and prototype in
`~/playground/2026_10_15_asqr-maskottchen/vorbereitung/header/`,
`compare/` and `prototype.diff`, switch `ASQR_PROTO_HEADER=einzug`):

- Variant 4, "title in the title line": the session's title sits next
  to `asqr` in the title line; `from`, `follows` and the intro hang
  indented below it.
- The intro takes the text colour instead of dim. This changes spec 7.1
  and the "Secondary" role in ADR 37.
- When the height is tight, the header gives way before the context
  pane drops to its one-line hint: first its blank lines, then intro
  lines.
- Jakob wavered between this and variant 3, "accent bar" (prototype
  switch `balken`). Keep variant 3 on record as the alternative if
  variant 4 does not satisfy in use.

## Design (decided by the lead)

- Variant 4 drops the queue name before it cuts the title, and cuts
  the title to one line. The empty queue keeps today's title line.
- Fixes a bug of today on the way: at 80×24 a `follows` id makes the
  title wrap, the tab rule lands on row 8, the tab area falls below the
  15 rows the context pane needs (ADR 30), and the pane shrinks to its
  hint. The new header must keep the rule at row 7 or above at 80×24,
  which the "give way" rule above does.

## Relations

- Relates to: [Show the title of the session a follow-up continues](01M51XE4NGWSJS9JW62AMCQBHJ-show-the-title-of-the-session-a-follow-up-continues.md)
