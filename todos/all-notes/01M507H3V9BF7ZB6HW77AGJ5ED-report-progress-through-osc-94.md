---
title: "Report progress through OSC 9;4"
kind: feature
component: tui
origin: discussion
tags: [ux]
---
# Report progress through OSC 9;4

Chosen by the user on 2026-10-15 in the review of the TUI's look. asqr
runs in a terminal of its own, often in a tab the person does not look
at. Its state shows inside the window, in the terminal title (ADR 40),
and through a desktop notification and the bell when a session arrives
(`src/tui/alert.rs`).

## Proposal

- In terminals that support it, report progress through the session
  with OSC 9;4, so the terminal's tab shows it.
- Inside tmux, send OSC 9;4 through the passthrough wrapper of
  `alert.rs`: tmux does not know the sequence. The title goes out plain
  there, since tmux takes it as the pane title (ADR 40).

## Decisions

- 2026-10-15, user: the title came first, as `asqr · {n} waiting` with a
  `--no-title` flag (done, ADR 40). Progress through OSC 9;4 follows only
  after checking which terminals support it.
- 2026-10-15, review: any session text written into an OSC drops
  control characters first, so an agent-written text cannot end the
  sequence.

## Open questions

- Which terminals support OSC 9;4, and whether asqr only sends it where
  it is known to work.
