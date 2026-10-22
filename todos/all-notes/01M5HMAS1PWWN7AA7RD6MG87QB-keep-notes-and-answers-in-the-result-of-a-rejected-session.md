---
title: "Keep notes and answers in the result of a rejected session"
kind: improvement
component: format
impact: high
origin: incident
tags: [ux]
---
# Keep notes and answers in the result of a rejected session

When a person rejects a session, the result carries only `status:
cancelled` and the `reason`. Every note and answer they typed before
rejecting is dropped, and `drafts/` is empty afterwards, so the agent
cannot get them back. Seen on 2026-10-22: the user wrote a long note on
the first question of a review session, rejected the session with the
reason "Schau dir die Notiz an Issue 01 an.", and the agent received the
reason pointing at a note that no longer existed anywhere.

## Problem

`SessionResult::cancelled` in `src/format/result.rs` takes only the
reason and leaves `answers` empty and `note` unset. In `src/tui/inbox.rs`
the finish code (around line 326) gets only the reason from
`Outcome::Rejected`, so nothing else can reach the result, and the
session's draft was gone from `drafts/` afterwards. Read in the code and
observed in real use (2026-10-22).

## Impact

A reason that refers to a note, which is the natural way to say "see
what I wrote there", reaches the agent without the note. The person has
to type it again in a second session, and nothing told them it was lost.

## Proposal

A `cancelled` result keeps what was entered: the session's `note` when
there is one, and `answers` for the questions that have an answer or a
note, in the same shape as in a submitted result. `status: cancelled`
already tells the agent that nothing was submitted, so there is no extra
flag. Rejecting stays one step, without a warning.

- `docs/spec.md` (around line 335) says "For `cancelled` and `error`,
  `answers` is empty", and `skills/asqr/SKILL.md` (around line 270)
  says "A rejected result has `reason` and no `note`". Both say instead
  that a rejected result may carry the note and the answers entered so
  far, which were not submitted.
- Tests first: a session with a note and one answered question,
  rejected, gives a result with the reason, the note and that one
  answer.

## Decisions

- 2026-10-22, user: keep what was entered in the `cancelled` result,
  without a warning on reject.
- 2026-10-22, user: the session's `note` and `answers` only for the
  questions with an answer or a note, without a `partial` flag.
- 2026-10-22, user: the change stays in format version 2.
