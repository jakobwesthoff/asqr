# 71. Let backspace clear the pick of a single question

Date: 2026-10-28

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md):
`backspace` clears the pick of a `single` question

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
own text answers a `single` only while it is the pick, and a draft marks
own text the person unpicked

## Context

- Every question is optional (ADR 28, spec sections 3.2 and 4.2).
- On a `single`, `enter` and the digits set the pick, and only `enter`
  on the own-answer row cleared the options. A `single` with a default
  therefore always came back answered, and one without a default could
  no longer be skipped once an option was picked, even by mistake.
- Writing a note on a defaulted question removes `defaulted` (spec
  section 4.2), so a note such as "not sure" next to the default read as
  a deliberate pick of it.
- Own text typed on a `single` counted whenever no option was chosen,
  and the review listed it next to a chosen option, which the result
  never sends.

## Decision

- `backspace` on the picked option's row of a `single` clears the pick,
  a default included. The question is then skipped; its note stays, and
  the answer carries no `defaulted`. The cursor stays on its row.
  `backspace` on any other row does nothing, so moving around never
  clears a pick by accident. A `multi` keeps clearing its options one at
  a time with `space` or a digit.
- `enter` on the option already picked keeps it picked: going back and
  pressing enter on the same answer again is ordinary and must not
  unpick.
- In a field `backspace` keeps deleting characters, in the own answer
  too: emptying its text is how a picked own answer goes.
- Typed text that is not selected is not the answer. When the person
  picked an option after typing own text and clears that option with
  `backspace`, the question is skipped and the text stays in its field,
  unpicked, with an empty mark. `enter` on the own-answer row picks it
  again, and so does editing it while no option is chosen, as typing
  does on a fresh question. Typing still never deselects a chosen
  option.
- A draft keeps unpicked own text as `custom` with
  `custom_unpicked: true` on its answer. Results never carry the field,
  and the result schema leaves it out.
- The review lists a `single`'s own text only where it is the answer:
  not next to a chosen option, and not while it is unpicked.
- The key bar names `backspace  clear` on the picked option's row of a
  `single`, right of `n  note`, so a narrow bar leaves it out first. The
  general help names it on the line of `L`:
  `L, backspace  the session list; clear the pick`.

## Consequences

- Every question can be skipped, a `single` with a default included.
- A submitted result lists a cleared `single` as `skipped: true`, with
  its note if it has one; a rejected result leaves it out unless it has
  a note. Unpicked own text is not sent.
- asqr 0.13.1 and earlier ignore `custom_unpicked` in a draft they
  resume, so the unpicked text counts there again.
- A cleared default leaves no selection in the draft, so `asqr withdraw`
  and `asqr ask --replaces` without `--force` do not count it as an
  answer, as with a `multi` whose default was toggled off.
