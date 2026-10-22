---
title: "Keep notes and answers in the result of a rejected session"
kind: improvement
component: format
status: needs-discussion
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

Two directions, not decided:

- A `cancelled` result also carries what was entered so far: the session
  `note` and `answers` for the questions that have an answer or a note,
  marked so the agent cannot mistake them for a submission (for example
  a `partial: true` flag, or answers only where something was entered).
  This changes the result format and the skill text that says a
  rejected result has `reason` and no `note`.
- The TUI warns on reject when notes or answers exist, offering to submit
  instead or to carry them into the reason.

## Open questions

- Is a partial answer list in a `cancelled` result a format version
  change for agents that read results strictly?
- Should the warning exist in addition to keeping the data?
