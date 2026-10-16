---
title: "Show the title of the session a follow-up continues"
kind: improvement
component: tui
origin: request
tags: [ux, field-test]
---
# Show the title of the session a follow-up continues

Found on 2026-10-16: Jakob answered a follow-up session and could not
tell which session it continued. The header shows
`follows: 01M51VZTB20Y36XSFZ43GMZMZF`, as spec section 3 asks, and an id
says nothing to a person.

## Decisions (Jakob, 2026-10-16, asqr session 01M51YYPA5SXZ8G7Y7M4PHG1F6)

- The header shows the title of the session followed.
- When that session cannot be found, or has no title, the header shows
  the id as the agent wrote it, as today.

## Design (decided by the lead)

- Look up a waiting session in the inbox first (its title is in the
  app's state), then the newest `<id>.<ulid>.json` in the queue's
  `archive/`, matching the id case-insensitively (spec 3.3); skip
  `.result.json` files and `invalid.*` entries.
- Look up only a value that is a valid id; `follows` is not validated
  as one (`src/format/session.rs:182`), so anything else shows as
  written.
- Resolve once per session load or reload and keep it in
  `SessionState`, not per frame.
- Spec: reword "The TUI never reads the outbox or the archive after a
  submit" (`docs/spec.md:191`) to cover the finish path only, and the
  `follows` wording in section 7.1 (`docs/spec.md:630`) and section 10
  (`docs/spec.md:1409`).
- Where the title goes depends on the header redesign; build both
  together or this one after it.
