---
title: "Option labels are cut off at the reading width and do not render Markdown"
kind: bug
component: tui
impact: high
origin: incident
tags: [ux, field-test]
---
# Option labels are cut off at the reading width and do not render Markdown

Seen on 2026-10-21 in a real review session (asqr 0.12.2): a `multi`
question whose options each carried a whole review remark in `label`.
Every option row ended mid-word at the reading width, although the
terminal was far wider, so the person could not read what they were
asked to pick. The labels also showed backticks raw, since labels are
not Markdown. Labels are to wrap and render Markdown like every other
text field, and the skill is to tell agents what a label is for.

## Problem

`src/tui/render/content.rs`, the `Row::Option` arm and `option_lines`:
the head (pointer, mark, number, label) is one `Line` built from a
single `Span` holding `option.label`. Nothing wraps it, and the row is
cut at the edge of the question pane, which never gets wider than
`READING_WIDTH` (100, `src/tui/render/panes.rs`). No ellipsis marks the
cut. Only the description goes through `field_lines`
(`src/tui/render/text.rs`) and so wraps and renders Markdown.

This matches the spec as written: `docs/spec.md` section 3.1 lists
`intro`, question `text` and option `description` as the Markdown
fields, and section 3.2 gives only the description "any length shown in
full and wrapped". Neither the validator nor the agent skill
(`skills/asqr/SKILL.md`) says that a label should be short.

Read in the code and seen in the screenshot of the session, not
reproduced in a test yet (2026-10-21).

## Impact

Text the person has to decide on is silently hidden. Picking an option
by its first 80 characters is guessing.

## Decisions

- 2026-10-21, user: labels render Markdown like the other text fields
  and wrap when they do not fit. A label never gets cut, however little
  room there is.
- 2026-10-21, user: the skill describes what a label is for and points
  agents to the option's `description` or `context` for longer text.
- 2026-10-22, user: the result carries option ids only, so labels show
  outside the option rows only in the review tab's one-line summary
  (`answer_summary` in `src/tui/render/parts.rs`). There a label
  renders its inline Markdown as in the option row.

## Suggested fix

- Render the label through the same Markdown path as the description,
  wrapped to the columns left after pointer, mark and number, with
  continuation lines under the label's start.
- Rework the description column: today it sits after the widest label
  (`label_column`). With wrapped labels, the description goes below the
  label once the labels take too much of the width.
- Spec: add option `label` to the Markdown fields in section 3.1, state
  in 3.2 that it wraps, and update the option layout in section 7.
- Skill: a label is the short handle the person picks by; longer text
  goes into `description` or the option's `context`.

## Open questions

- Where the switch from beside to below happens for descriptions when
  labels wrap.

## Done when

- A label longer than the pane wraps and is readable in full, in a
  pane of any width.
- Inline Markdown in a label (code span, emphasis) renders as it does in
  a description.
- Snapshot tests in `src/tui/render/screen.rs` cover a long label and a
  label with Markdown.
- Spec and skill describe labels as above.
