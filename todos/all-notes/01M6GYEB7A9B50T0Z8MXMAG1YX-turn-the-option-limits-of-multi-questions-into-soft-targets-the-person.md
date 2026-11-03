---
title: "Turn the option limits of multi questions into soft targets the person can exceed"
kind: improvement
component: tui
origin: request
tags: [ux]
---
# Turn the option limits of multi questions into soft targets the person can exceed

A `multi` question's `max` is a hard limit today: once that many options
are picked, the TUI refuses the next pick with "at most N options"
(`src/tui/session.rs`, `toggle`, seen 2026-11-03). The person answering
cannot pick more, even when they have a reason to. `min` is already
only shown as a hint (`src/tui/render/parts.rs`).

The user wants option counts to work like the length guide of typed
answers: the agent gives target ranges and warning levels, the TUI shows
them through colour and warnings, and the person can always go past
them.

## Decisions

- 2026-11-03, user (in the antiq session, verbatim): "we actually dont
  want to allow a hard limit on multi selects, like with text input
  different barrier lengths should be supplied by the agents, which are
  shown through coloring and warnings, but can always be overridden by
  the user answering the asqr session".

## Notes

- Typed answers already have this: `length` with `target: [low, high]`
  and `warn` (`src/format/session.rs`, struct `Length`), shown as a
  counter that never refuses input.
- The session format, its validation (`src/format/validate.rs`, the
  `min`/`max` checks), the skill text ("`min` and `max` count the picked
  options") and the TUI hint change together. Whether `min` and `max`
  keep their names with soft meaning or give way to a field shaped like
  `length` is open.
- It came up while answering asqr session 01M6GY8B84B32H0YQB6JBC2N9T
  in the antiq design session, which had `max` on two questions.
