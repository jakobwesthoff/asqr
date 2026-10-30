---
title: "A cleared default does not count as an answer in a draft"
kind: bug
component: queue
impact: low
origin: review
---
# A cleared default does not count as an answer in a draft

Found in a plan review on 2026-10-28. When the person clears a
question's default, the draft keeps no trace of it. `asqr withdraw` and
`asqr ask --replaces` without `--force` then treat the draft as
unanswered, and the person's decision goes with the session.

## Problem

`person_entered` in `src/queue/drafts.rs`, which `draft_has_answer`
applies to every answer, counts a selection without the `defaulted`
flag, typed text that is not blank, or a note that is not blank. A
default cleared with `backspace` on a `single` (ADR 0071), or toggled
off on a `multi`, leaves an answer with no selection and no `defaulted`
flag (`SessionState::answers` in `src/tui/session.rs`), so nothing in
the draft says the person touched the question.

`withdraw_locked` in `src/queue/withdraw.rs`, which serves
`asqr withdraw` and the old session of `asqr ask --replaces`, refuses
only when `has_answered_draft` says yes and `--force` is not given. The
withdrawal then deletes the draft. The withdrawn result's unsent draft
comes from `rejected_answers`, which leaves out a skipped answer without
a note, so the cleared default does not show there either.

ADR 0071 records this under Consequences, and spec section 4.3 defines
"has an answer" by selections the person made, text and notes. The same
section gives the reason a selection counts: "it is still work a
replacement would throw away". A cleared default is such work too.

The same gap shows when a session starts from its draft, as after a
restart of asqr. An untouched question without a default and a question
whose default the person cleared leave the same answer in a draft: no
selection, no `defaulted`. `restored` in `src/tui/session.rs` restores
both as touched, so a default the session file gained since is not
picked on either. A running session's reload knows which questions were
touched and has no such gap.

Read in the code on 2026-10-28, not reproduced.

## Impact

An asker that withdraws or replaces the session without `--force` drops
the person's decision to skip a defaulted question without being
refused, and a replacement starts with its defaults selected again. A
note or any other answer in the draft prevents it. After a restart, a
default added to an untouched question without one stays unpicked.

## Suggested direction

Give the draft a mark for a question the person changed, as
`custom_unpicked` marks unpicked own text in drafts only, count it in
`person_entered`, and restore a question without it as untouched. The
change amends spec section 4.3 and ADR 0071.

## Decisions (Jakob, 2026-10-30, asqr session 01M6628SFCCK00ENY6YV0XQZR1)

- A cleared default counts as the person having started: `asqr withdraw`
  and `asqr ask --replaces` without `--force` refuse, and with `--force`
  the person is told the asker withdrew the session.
- A restored draft preselects a default the session file gained on a
  question the person never touched, as a running session's reload does.
