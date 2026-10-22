---
title: "Separate the question from the context pane with a ledge"
kind: improvement
component: tui
origin: request
tags: [ux]
---
# Separate the question from the context pane with a ledge

On a wide terminal the question and its context pane stand side by side
with one blank column between them (`SEPARATOR` in
`src/tui/render/panes.rs`), and the pane's header is bold text in the
text colour, like the Markdown headings under it. The question's lines
end at that blank column and the pane's lines start right after it, so
the two sides read as one line, and nothing marks where the pane
begins. Raised on 2026-10-22 with a screenshot from real use; the design
below was chosen on 2026-10-22 from three rounds of rendered mockups.

## Proposal

The pane gets a ledge: its header becomes a dim rule with the entry's
name set into it, which turns down into a dim left edge, and a bottom
rule with the position closes it.

Side by side:

```
 …is there and what removing it ╭─ Why it is there, and what removing it changes ──── ─
 changes (details in the pane). │ History
 In short: connect() was a      │
 placeholder in the first SDK   │ • library-node-apisdk ca32891 (2018-11-05, …
 version of 2018 and has …      │   …
                                ╰───────────────────────────────── 1–31 of 58 · tab ─
```

One under the other:

```
 Notes: –

 ── Why it is there, and what removing it changes ─────────────────────────── ──
 History

 • library-node-apisdk ca32891 (2018-11-05, "added first steps to a glorious …
 ───────────────────────────────────────────────────────── 1–31 of 58 · tab ──
```

- **Header:** a dim rule with the name in bold, one blank on each side
  of it: `╭─ name ───── ─` side by side, `── name ───── ──` one under
  the other. The position leaves the header, so the name has the
  header's width less the rule pieces.
- **Left edge:** side by side, a dim `│` runs down from the `╭` to the
  bottom rule. The content starts two columns right of it. The gap
  between question and pane stays one blank column, so the question
  keeps its width. One under the other there is no left edge and the
  content starts at the area's left.
- **Bottom rule:** the position from the header (ADR 0031: `4–15 of
  120`, with `· tab` while the pane does not have the focus) moves into
  a rule in the pane's last row: `╰──── 1–31 of 58 · tab ─` side by
  side, `──── 1–31 of 58 · tab ──` one under the other. The position
  text stays dim.
- **One under the other:** the first row of the context area stays
  blank, so a blank row stands between the question and the header
  rule.
- **Focus:** the focused pane's name takes the focus fill, blanks
  included, and its header rule, left edge and bottom rule turn yellow.
  The position stays dim. This replaces the focus fill over the whole
  header row (ADR 0031 as amended by ADR 0037). Under `NO_COLOR`,
  `strip_colours` in `src/tui/render/theme.rs` resets named colours and
  keeps modifiers, so the lines lose their yellow and the reversed fill
  on the name alone shows the focus.

## Costs

- Side by side, the pane's body is two columns narrower than the pane's
  area (the edge and the blank after it). At `MIN_CONTEXT_WIDTH` of 40
  the body is 38 columns wide.
- The bottom rule takes the body's last row in both arrangements, and
  one under the other the blank row takes one more.

## Docs to change

- A new ADR that amends ADR 0030 and ADR 0031 for the header, the edge
  and the bottom rule, and ADR 0037 for the focus.
- `docs/design-guide.md`: the Focus row of the role table names the
  focused context pane's header as a fill; it gains the yellow lines.

## Open questions

- The bottom rule when the content fits and the header shows no
  position today: a rule without text, or no bottom rule.
- The full-screen view (`z`) draws the pane over the whole tab area,
  header included (ADR 0031): whether it takes the ledge too.
- Whether `MIN_CONTEXT_WIDTH` should count the edge, so the body keeps
  at least 40 columns, which would raise the narrowest tab area that
  holds both side by side (`MIN_BESIDE_WIDTH`, 101 columns).
- Where the red `✗` of an entry asqr cannot show sits in the ruled
  header (design guide, Error row: the `✗` before its name in the pane's
  header).

## Decisions

- 2026-10-22, user: the ledge, from these mockups considered: four blank
  columns with a ruled header, a vertical rule between the sides, the
  tab bar's rule branching down, the header set into the tab bar's
  rule, a thick bar down the pane's edge, a header bar in the theme's
  black, a tab for the entry, a rounded frame, the vertical rule with a
  ruled header, and the ledge with three blank columns before it.
- 2026-10-22, user: the focused pane fills the name and turns its lines
  yellow, over filling the name alone or the whole header row.
- 2026-10-22, user: the position goes into a bottom rule, over keeping
  it in the header, where the ledge cuts the name sooner, and over dim
  text under an open edge, right or left.
- 2026-10-22, user: one under the other, a header rule only, without
  the `╭` and the left edge.

## Relations

- Relates to: [Show several context items per question in the context pane](01M4XFJG5ZYVHJ12G5NJA5970W-show-several-context-items-per-question-in-the-context-pane.md), whose tab strip goes into the header line this todo turns into a rule
