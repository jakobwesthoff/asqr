---
title: "A restored draft keeps own text on a question that no longer takes own text"
kind: bug
component: tui
origin: review
tags: [ux]
---
# A restored draft keeps own text on a question that no longer takes own text

Found in a plan review on 2026-10-28. When a replaced session file turns
`custom` off for a question, the own text the draft held for it stays in
the answer. The question has no own-answer row any more, so the person
cannot edit or clear the text, and a submit sends it.

## Problem

- `restored` in `src/tui/session.rs` keeps the draft's `custom` text for
  every `single`, `multi` and `text` question and drops it only for an
  info item. It does not look at whether the question still enables
  `custom`.
- `rows_of` in the same file adds the own-answer row only while `custom`
  is enabled (`Custom::is_enabled`).
- `result_answers` and `shaped` in `src/format/answer.rs` do not look at
  `custom` either. A `single` with no option chosen sends the text as
  its answer, and a `multi` sends it next to its options.
- The review's one-line summary (`answer_summary` in
  `src/tui/render/parts.rs`) shows the text in quotes. That is the only
  place it appears.

Spec section 4.3 says that on restore anything that no longer fits is
dropped. Spec section 5.5 says a file dropped into the inbox by hand
replaces whatever is there, draft or not, and `Inbox::load` in
`src/tui/inbox.rs` then builds the session from the new file and the old
draft. `asqr ask` with the same id refuses while the draft has typed
text, as that counts as an answer, so the way in is a file dropped by
hand.

Read in the code on 2026-10-28, not reproduced.

## Impact

The asker receives own text for a question it now asks without one,
written for an earlier version of the session, and the person has no row
to take it back.

## Suggested fix

In `restored`, drop `custom` and `custom_unpicked` for a `single` or
`multi` whose question does not enable `custom`. Let `result_answers`
leave out own text on such a question as well, so no other path sends
it.
