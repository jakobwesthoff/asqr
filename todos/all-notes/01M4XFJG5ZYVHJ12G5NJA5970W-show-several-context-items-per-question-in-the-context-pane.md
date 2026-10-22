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
  pane has the focus; `ctrl-n`/`ctrl-p`, which work even in a field.
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
