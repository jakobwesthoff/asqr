---
title: "A cleared default does not count as an answer in a draft"
kind: bug
component: queue
status: needs-discussion
impact: low
origin: review
---
# A cleared default does not count as an answer in a draft

Found in a plan review on 2026-10-28. When the person clears a
question's default, the draft keeps no trace of it. `asqr withdraw` and
`asqr ask --replaces` without `--force` then treat the draft as
unanswered, and the person's decision goes with the session.

## Problem

`draft_has_answer` in `src/queue/drafts.rs` counts a selection, typed
text that is not blank, or a note. A default cleared with `backspace` on
a `single` (ADR 0071), or toggled off on a `multi`, leaves an answer
with no selection and no `defaulted` flag (`SessionState::answers` in
`src/tui/session.rs`), so nothing in the draft says the person touched
the question.

`withdraw_locked` in `src/queue/withdraw.rs`, which serves
`asqr withdraw` and the old session of `asqr ask --replaces`, refuses
only when `has_answered_draft` says yes and `--force` is not given. The
withdrawal then deletes the draft. The withdrawn result's unsent draft
comes from `rejected_answers`, which leaves out a skipped answer without
a note, so the cleared default does not show there either.

ADR 0071 records this under Consequences, and spec section 4.3 defines
"has an answer" by selections, text and notes. The same section gives
the reason a selection counts: "it is still work a replacement would
throw away". A cleared default is such work too.

The reverse also holds: an untouched default is a selection, so it
counts. A draft is written on every change (spec section 4.3), so a
draft whose only content is untouched defaults, for example after a note
was typed and deleted again, counts as answered and blocks a withdrawal
without `--force`.

Read in the code on 2026-10-28, not reproduced.

## Impact

An asker that withdraws or replaces the session without `--force` drops
the person's decision to skip a defaulted question without being
refused, and a replacement starts with its defaults selected again. A
note or any other answer in the draft prevents it.

## Suggested direction

Give the draft a mark for a question the person changed, as
`custom_unpicked` marks unpicked own text in drafts only, and count it
in `draft_has_answer`. Whether an untouched default keeps counting
belongs to the same decision. Either change amends spec section 4.3 and
ADR 0071.
