---
title: "Show several context items per question in the context pane"
kind: feature
component: tui
status: needs-discussion
origin: discussion
tags: [ux]
---
# Show several context items per question in the context pane

Phase 2 of the context pane, split off on 2026-10-14 while discussing
phase 1. A question or option carries several context items, for
example the diffs of three files, or a diff and a screenshot. The pane
shows one item at a time with a tab strip at its top, and the person
switches between items quickly (user, round 1).

## Proposal

- Format: phase 1 defines `context` in format version 2 as a list
  limited to one entry (user, 2026-10-14). Phase 2 lifts the limit; no
  format change.
- The tab strip names each item by its title, or by its file name when
  it has none, and scrolls like the question tab bar when it does not
  fit.
- Each item keeps its own scroll position while the person switches.

## Open questions

- Keys for switching items, usable on non-US layouts (no `[`/`]`)
  and from the question pane as well as the focused pane. Candidates:
  `tab` cycling through the question and every item; digits while the
  pane has the focus; `ctrl-n`/`ctrl-p`. In a text field
  `ctrl-n`/`ctrl-p` move the cursor by a row (`src/tui/text_field.rs:211-212`,
  spec 7.7), so they work on the question rows and in the pane, not in a
  field. Digits pick options on the question rows (`src/tui/session.rs`,
  `command`), so they are free only while the pane has the focus.
- How an option's items combine with the question's while the cursor is
  on that option: instead of them (as option images do today) or in
  front of them.
- A limit on items per question, if any.

## Where the pane leaves it

The context pane (ADR 31) shows one entry under a header rule with the
entry's name set into it (ADR 54), which is where a tab strip can go;
the scroll position sits in a bottom rule. `SessionState` keeps a scroll position per
`EntryKey` (question and option index); an item index in that key gives
each item its own position. `Viewers` keeps one viewer per shown entry
under the same key plus the session id.

## Questions put to Jakob on 2026-10-30, kept open

Jakob chose "later, keep it open" for all four. Options, the lead's
recommendation first:

- Switch key: `ctrl-n`/`ctrl-p` (free on the question rows and in the
  pane, same place on US, German and French layouts); digits while the
  pane has the focus; `,` and `.`.
- Where the strip sits: in the pane's header rule in place of the
  entry's name, so no row is lost; a row of its own below the header.
- Items per question: up to five, refused above; no limit; warn above
  five.
- An option's items: replace the question's while the cursor is on the
  option, as today; come in front of the question's.

Not yet asked: whether switching stops or wraps at the first and last
item, which item shows when the cursor returns to a row (the last shown
or the first), and how an item without a title is named in the strip.
The limit `MAX_CONTEXT_ENTRIES = 1` sits in `src/format/validate.rs:26`,
and `skills/asqr/SKILL.md` tells agents the same.
