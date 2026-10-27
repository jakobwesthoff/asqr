---
title: "Show context without asking a question through an info item"
kind: feature
component: format
status: needs-discussion
origin: discussion
tags: [ux]
---
# Show context without asking a question through an info item

Came up on 2026-10-27 while an agent closed a long piece of work through
asqr: it wanted to show the person the results so far (a table of
releases, what changed beyond the tickets, mishaps along the way) before
the actual questions. asqr knows only the kinds `single`, `multi` and
`text`, so the agent wrapped the results in a stand-in question with the
options "Passt" and "Rückfragen". The person agreed that a dedicated item
for this would be better (user, 2026-10-27).

## Goal

A session can hold an item that only shows something: text and context
entries like a question has, but no answer to give. The person reads it,
moves on, and can still attach a note to it. The intro stays short; the
info item carries what does not fit there.

## Proposal

Not decided, a first idea:

- Format: `kind: "info"` with `header`, `text` and `context`, no
  `options`, no `custom`, no `length`; `note` works as on questions.
- Result: the item appears in `answers` with no `selected`, only a
  `note` when the person wrote one, or is left out when it has none.
- TUI: shown in the question list like a question, but never counted as
  unanswered, so submitting does not ask about it.

## Open questions

- Should an info item count as "seen" in the result, so the agent knows
  the person reached it?
- Is the name `info` right, or something like `notice` or `section`?
