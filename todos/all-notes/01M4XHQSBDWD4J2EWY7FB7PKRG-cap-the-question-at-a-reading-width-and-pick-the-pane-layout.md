---
title: "Cap the question at a reading width and pick the pane layout automatically"
kind: feature
component: tui
horizon: next
origin: discussion
tags: [ux, milestone/v0-11]
---
# Cap the question at a reading width and pick the pane layout automatically

Step 2 of the context pane plan (design settled with the user on
2026-10-14). On wide terminals, question texts, option descriptions and
notes run across the whole width and are hard to read. This step caps
them at a reading width and introduces the layout that places a context
area beside or below the question. Today's images move into that area.

## Proposal

The plan's Design section, *Layout*, has the full rules. In short:

- A reading width cap for the question of about 100 to 110 columns
  (start with 100, tune by trying it), left-aligned, with or without a
  context area.
- Beside: half and half until the question reaches the cap, from 101
  columns (question at least 60, context at least 40, one separator
  column). Below: the question takes the rows it needs up to half the
  height, the context area the rest, at least 8 rows.
- `--layout auto|beside|below` with an environment variable (ADR 0015
  style). `auto` decides by the visible shape W ÷ 2H with hysteresis
  (beside from 1.1, below under 0.9) when both fit.
- A question without context reflows to the wider width it can get.
- The header's title and intro wrap at the reading width; the tab bar
  keeps the full width. The intro still shows at most three lines, the
  cut reads `… i: more`, and `i` shows the whole intro in an overlay.
- Images (question and option images as today, or `context` image
  entries once step 1 landed) are drawn into the context area. This
  replaces `IMAGE_BESIDE_WIDTH` and the 3:2 and one-third rules in
  `draw_tab` (`src/tui/render/screen.rs`).
- Spec section 7.1, the help overlay, an ADR, and the skill's
  description of the intro (`i`).

## Decisions

- 2026-10-14, user: the environment variable is `ASQR_LAYOUT`.
- 2026-10-14, user: the agent decides where the hysteresis state lives,
  as long as a redraw without a resize keeps the current layout.

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
