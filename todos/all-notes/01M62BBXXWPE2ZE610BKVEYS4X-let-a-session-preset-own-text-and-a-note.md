---
title: "Let a session preset own text and a note"
kind: feature
component: format
status: needs-discussion
origin: review
---
# Let a session preset own text and a note

A session can preset an option with `"default": true`, but nothing else
the person can enter. When an agent withdraws a session and asks a
replacement, the withdrawn result's `draft` carries the person's own
text (`custom`) and notes too, and the replacement cannot give them
back.

## Problem

The format has no field that fills a question's own-answer field, a
`text` question's field, or a question's note before the person types.
`default` exists only on options (`schema/session.v2.json`). The skill
(`skills/asqr/references/withdrawing.md`) therefore
tells an agent to quote typed text and notes in the replacement
question's text, so the person can paste or retype them. Typed text is
the part of a draft that costs the most to redo.

Found in the final review before the release after 0.13.1, on
2026-10-28 (reviewer finding O4); the user asked for this todo on the
same day.

## Open questions

- The field's name and where it sits: on the question (own text, note)
  or in a separate preset block.
- Whether a preset counts as an untouched default does: the asker's
  value, not the person's input, until the person changes it (see how
  `defaulted` works for options, spec section 4.2).
- How the TUI marks preset text, next to the `◀ default` marker of ADR
  76.
- Whether it is meant for replacement sessions only or for any session.

- Which fields: own text, a `text` question's answer, a question's
  note, or fewer.
- Whether a preset own text on a `single` starts as the pick, as a
  default option does.

## Decisions

- **Parked (Jakob, 2026-10-30, in the chat).** The feature needs more
  thinking about how to keep an agent from misusing it: a preset text
  looks like the person's own words, and an agent could fill in answers
  the person never wrote.
- **Resumed with a safeguard (Jakob, 2026-10-31, in the chat).** The
  feature goes ahead only with a result flag that marks a preset the
  person sent unchanged, as `defaulted` does for options, and with a
  start in which the preset is not picked. Rejected: keeping it parked.
  The detail questions above follow.

## Checked on main 1dcae03 (lead, 2026-10-30)

- `note` on a question is a boolean in format version 2; a string there
  is a shape error (`src/format/session.rs:251-257`). Todo
  01M61Z3MJBAWT63Y2RYEKSMJD6 removes that field within version 2
  (decided 2026-10-31), and a file that still sets `note` then gets a
  warning that it is ignored; a note preset needs another name, or a
  deliberate change of that warning.
- `person_entered` (`src/queue/drafts.rs:85-90`) counts any own text and
  note, whatever their flags. A preset stored without a flag would count
  as the person's input and block a withdrawal without `--force`, so the
  format, `person_entered` and `restored` (`src/tui/session.rs`) change
  together.
- `defaulted` is set for selections only (`src/tui/session.rs:869`).
- The file does not record that a session replaces another
  (`ask --replaces` is a flag), so "replacement sessions only" cannot
  be checked from the file alone.
