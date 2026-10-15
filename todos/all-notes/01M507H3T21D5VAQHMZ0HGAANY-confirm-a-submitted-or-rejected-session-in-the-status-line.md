---
title: "Confirm a submitted or rejected session in the status line"
kind: feature
component: tui
origin: discussion
tags: [ux]
---
# Confirm a submitted or rejected session in the status line

Chosen by the user on 2026-10-15 in the review of the TUI's look. When the
person submits or rejects a session, the session leaves the screen and the
next one, or the empty queue, shows at once. Nothing on screen says that
the result was written; `finish` in `src/tui/inbox.rs` does not raise a
notice on success.

## Proposal

A notice in the status line until the next key, through the existing
`App::notice`:

    ✔ sent "Release 0.2.0" · 1 answered, 2 skipped
    ✗ rejected "Release 0.2.0"

The counts come from what the review tab already shows. The tick for a sent
session takes the success colour of the colour roles; the rejected line
stays neutral.

## Decisions

- 2026-10-15, user: notices get kinds first (information, warning, error,
  success), and one ADR covers the kinds and this confirmation.
- 2026-10-15, user: the confirmation stays until the next key or paste,
  also over the next session and over the empty queue.
