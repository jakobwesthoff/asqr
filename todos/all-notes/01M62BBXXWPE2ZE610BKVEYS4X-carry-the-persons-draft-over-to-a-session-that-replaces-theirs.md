---
title: "Carry the person's draft over to a session that replaces theirs"
kind: feature
component: queue
status: needs-discussion
origin: review
---
# Carry the person's draft over to a session that replaces theirs

When an agent withdraws a session the person had started and asks a
replacement with `ask --replaces <old>`, everything the person entered
is lost from the screen: it sits in the `draft` of the withdrawn result,
and the replacement starts empty. asqr is to carry that draft over into
the replacement itself, so the person's own text, text answers, notes
and picks stay where they were, and no agent ever writes text into a
field the person is meant to fill.

## Problem

The format can preset only options (`"default": true`). The skill
(`skills/asqr/references/withdrawing.md:27-40`) therefore tells an agent
to set `default` on the options the person had picked and to quote typed
text and notes in the replacement question's text, so the person can
paste or retype them. Typed text is the part of a draft that costs the
most to redo. Found in the final review before the release after 0.13.1,
on 2026-10-28 (reviewer finding O4).

## Decisions

- **Parked (Jakob, 2026-10-30, in the chat).** Presets written by the
  agent need more thinking: a preset text looks like the person's own
  words, and an agent could fill in answers the person never wrote.
- **Resumed (Jakob, 2026-10-31, in the chat),** first on the condition of
  a result flag for presets sent unchanged. Asked about the fields,
  Jakob restated the goal: what the person has entered must not be lost
  when an agent reworks a session, and an agent must not be able to
  write arbitrary text into these fields, since that defeats their
  purpose.
- **asqr carries the draft over, automatically (Jakob, 2026-10-31, in
  the chat; the lead's proposal).** On `ask <new> --replaces <old>`, the
  withdrawn session's draft becomes the replacement's draft. It is
  matched by question id and option id under the rules a draft is
  restored by today (spec 4.3): what no longer fits is dropped. The
  agent writes no text; it steers only through the ids. A question kept
  under its id keeps the person's input, a question under a new id
  starts fresh, so an agent gives a question whose meaning changed a new
  id. Rejected: carry-over only with a flag such as `--keep-draft`,
  which an agent can forget; presets written by the agent with a result
  flag. This replaces the presets and the skill's quoting rule.

## Open questions

- Product (for Jakob): whether the carried-over entries get a mark in
  the TUI (for example `◀ draft`, as the replay marks a withdrawn draft's
  picks in a finished session shown again, spec 7.11), or show as a restored draft of the same
  session does today, without one.
- Technical (for the lead): carried-over entries are the person's input,
  so `person_entered` (`src/queue/drafts.rs:96`) counts them and a
  further `--replaces` needs `--force`; check that this is what the
  withdrawal rules want. Whether the hint after `ask --replaces` ("what
  they entered is in the `draft` of its result", spec 6.7) changes
  to say what was carried over. Whether `ask` with the same id, which
  replaces a waiting session only while nobody has started on it, needs
  anything (it has no draft to carry). How the draft is written under the
  drafts lock that `--replaces` already holds for the withdrawal and the
  drop (spec 5.5).
- The skill's `withdrawing.md` drops the advice to set `default` on
  earlier picks and to quote typed text, and says to keep the ids of the
  questions and options whose earlier answers should stay.

## Checked on main 1dcae03 (lead, 2026-10-30)

- `note` on a question is a boolean in format version 2 today; todo
  01M61Z3MJBAWT63Y2RYEKSMJD6 removes that field within version 2. The
  carry-over restores a note from the draft, which has its own `note` on
  each answer and does not depend on that field.
- `restored` (`src/tui/session.rs:1965`) decides whether a restored
  answer counts as touched; an untouched default restores as the file's
  defaults.
- The file does not record that a session replaces another
  (`ask --replaces` is a flag), so the carry-over runs in `ask`, which
  has both sessions at hand.
