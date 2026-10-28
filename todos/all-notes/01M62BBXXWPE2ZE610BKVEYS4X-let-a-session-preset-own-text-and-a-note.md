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
(`skills/asqr/SKILL.md`, the part on withdrawing a session) therefore
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
  `defaulted` works for options and spec section 4.3).
- How the TUI marks preset text, next to the `◀ default` marker of ADR
  76.
- Whether it is meant for replacement sessions only or for any session.
