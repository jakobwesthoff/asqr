---
title: "Colour the TUI by the roles of the design guide"
kind: improvement
component: tui
origin: discussion
tags: [ux]
---
# Colour the TUI by the roles of the design guide

Decided with the user on 2026-10-15 after comparing six prototyped colour
variants in four terminal themes. Today the TUI marks nearly everything the
same way, with reversed video: the title line, the current tab, the row
under the cursor, the focused context pane's header, the submit button and
every key in the key bar. Up to nine equally loud blocks stand on one
screen, and colour only appears for states (counter, diff, notices,
highlighted code). The section "Terminal interface" of
`docs/design-guide.md` now gives every role its terminal colour.

## Proposal

As in the chosen prototype variant:

- Title line without a fill: `asqr` bold cyan, the queue path dim, the
  count normal.
- Current tab bold cyan, with the rule under the tab bar heavy (`━`) and
  cyan under it and thin and dim (`─`) elsewhere. Unanswered tabs dim.
- The row under the cursor, the focused field's head, the current review
  row, the current entry of the session list and the focused pane header
  as a yellow fill. In the prototype that is `fg(Yellow)` plus
  `REVERSED`, which keeps the cursor detection in `wrap_rows` and
  `mark_cut` (`src/tui/render/screen.rs`) working, since it looks for
  `REVERSED`.
- Chosen marks bold cyan, option numbers dim.
- Submit as `[ Submit ]`, brackets included, always in a cyan fill, with
  the pointer before it yellow when the cursor is on it; the review's
  counts with bold numbers and dim words.
- Key bar without fills: the key bold cyan, its description dim.
- Help and session list with rounded borders in cyan, bold titles, the
  help's keys in cyan and one column of padding inside its border.
- While the context pane has the focus, the question's current row shows
  its pointer and bold text without a fill, so one focus fill shows at a
  time. Today, and in the prototype, both the row and the pane header are
  filled then.

## Decisions

- 2026-10-15, user: inline code in Markdown stays yellow (ADR 35), next to
  the yellow focus fill.
- 2026-10-15, user: one focus fill at a time, as above.
- 2026-10-15, user, after a review of the plan: the submit button keeps
  its brackets inside the permanent cyan fill, so it stays apart from the
  focus fill when `NO_COLOR` turns both into plain reversed blocks. The
  design guide names it as the one fill that is not the focus.
- 2026-10-15, user: bold stays on coloured text. Terminals that draw bold
  colours as bright show it grey in Solarized; the ADR records that.
- 2026-10-15, review: the fill stays `fg(Yellow)` plus `REVERSED`. A
  `bg(Yellow).fg(Black)` fill vanishes under `NO_COLOR`, and with the
  themes' own black it reads no better (2.4:1 in Catppuccin Latte, worse
  than today in Catppuccin Mocha). On light themes the fill has about
  2.3:1 (Latte) to 3:1 (Solarized Light); the ADR records that.
- 2026-10-15, review: under the fill a row is one colour and dim is
  lifted; one helper patches every span of a filled line, since a span's
  own colour wins over its line's style.
- 2026-10-15, review: no production code finds the current row by its
  `REVERSED` style, so dropping the fill while the pane has the focus is
  safe.

## Open questions

- Focus and warning both use yellow. Whether that reads clearly enough
  where a refused key's notice shows next to a focused row.

## Relations

- The roles this change fills in are in `src/tui/render/theme.rs`.
