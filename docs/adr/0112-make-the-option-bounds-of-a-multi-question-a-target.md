# 112. Make the option bounds of a multi question a target

Date: 2026-11-20

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
`min` and `max` of a `multi` are a target range, and a selection outside
it answers the question

Amends [38. Give notices kinds and confirm a finished session](0038-give-notices-kinds-and-confirm-a-finished-session.md):
a pick past `max` is no longer a refused key

## Context

- A `multi` question's `max` was a hard limit: once that many options
  were picked, the TUI refused the next pick with "at most N options".
  A selection below `min` or above `max` counted as no answer, so the
  result reported the question as skipped.
- Typed answers already guide without refusing: `length` gives a target
  range and a `warn` level, shown as a counter that never refuses input
  (ADR 8).
- On 2026-11-03 the user, answering a session with `max` on two
  questions, said: "we actually dont want to allow a hard limit on multi
  selects, like with text input different barrier lengths should be
  supplied by the agents, which are shown through coloring and warnings,
  but can always be overridden by the user answering the asqr session".
- On 2026-11-20 the user chose, of the options put to him, to keep
  `min` and `max` with a soft meaning over a new field shaped like
  `length`, to let a selection outside the range count as answered, and
  to keep more defaults than `max` an invalid session.

## Decision

- `min` and `max` keep their names and their place in format version 2.
  They are a target range for the number of picked options.
- The TUI never refuses a pick. Once a `multi` with `min` or `max` has a
  pick, the hint under its question adds the count, such as "pick up to
  3 items · 4 picked", green within the range and yellow outside it.
- Any selection answers a `multi`. A selection below `min` or above
  `max` comes back in `selected` like any other, never as skipped.
- More defaults than `max` stays a validation error, as do `min` above
  `max` and either above the number of options.
- The skill tells agents that the count may fall outside their range.

## Consequences

- Session files written for earlier versions keep working unchanged;
  their `max` now guides instead of limiting.
- An agent that uses `max` as a hard cap, such as for the candidates of a
  pipeline step, must handle a longer list or ask again.
- A result no longer tells a selection outside the range apart from one
  within it; the agent counts `selected` itself.
- Option counts have one warning level, unlike `length`, which has two.
