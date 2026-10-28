---
title: "Let the person clear the pick of a single question"
kind: bug
component: tui
origin: review
tags: [ux]
---
# Let the person clear the pick of a single question

Found on 2026-10-28 in the plan review of the skill's task patterns and
checked in the code by the lead. The spec (sections 3.2 and 4.2) and the
skill ("Every question is optional: the person may skip any of them")
promise that every question can be skipped, but a `single` question
cannot go back to having no option picked.

## Problem

`SessionState::pick` in `src/tui/session.rs` sets the selection of a
`single` question to the picked option; only `pick_own` clears it, and
only on a question with `custom`. Spec section 7.6 has no key that
unpicks. So:

- A `single` with a `default` always comes back with an option: the
  default, or another one the person picked. The person cannot skip it.
- A `single` without a default cannot be skipped any more once the
  person has picked an option, even by mistake.
- Writing a note on a defaulted question removes `defaulted` from the
  result (spec 4.2), so "not sure, see my note" reads to the asker as a
  deliberate pick of the default.

Read in the code, not reproduced in a test yet (2026-10-28).

## Impact

The person cannot leave a question open that the agent may treat as
optional, and an agent reads a pick the person did not mean.

## Decided

The user decided on 2026-10-28 that `backspace` on a `single` question's
option rows clears its pick, so the question counts as skipped; `enter`
on the picked option keeps picking it, since going back and pressing
enter on the same answer again is ordinary and must not unpick. Text
fields keep `backspace` for deleting characters.

## Open questions

- Whether `backspace` also clears a `multi` question's picks at once, or
  only `single`.
- What happens to `defaulted` and to the note when the pick is cleared.
- The skill's patterns section advises an option such as "not yet" on a
  `single` with a `default`; that advice changes with the fix.
