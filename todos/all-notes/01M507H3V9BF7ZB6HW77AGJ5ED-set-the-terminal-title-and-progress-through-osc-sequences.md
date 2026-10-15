---
title: "Set the terminal title and progress through OSC sequences"
kind: feature
component: tui
origin: discussion
tags: [ux]
---
# Set the terminal title and progress through OSC sequences

Chosen by the user on 2026-10-15 in the review of the TUI's look. asqr runs
in a terminal of its own, often in a tab the person does not look at. Its
state shows only inside the window, plus a desktop notification and the
bell when a session arrives (`src/tui/alert.rs`).

## Proposal

- Set the window title with OSC 2, for example `asqr · 2 waiting`, and
  update it when the queue changes.
- In terminals that support it, report progress through the session with
  OSC 9;4, so the terminal's tab shows it.
- Write the title plainly, also inside tmux: tmux takes OSC 2 as the
  pane title (`allow-set-title`, on by default) and passes it to the outer
  terminal when `set-titles` is on (off by default). Only OSC 9;4, which
  tmux does not know, would go through the passthrough of `alert.rs`.

## Decisions

- 2026-10-15, user: the title comes first, as `asqr · {n} waiting`, and a
  `--no-title` flag switches it off, like `--no-notify` and `--no-bell`.
  Progress through OSC 9;4 follows only after checking which terminals
  support it.

- 2026-10-15, review: restoring the title is best effort. asqr pushes
  the title (`CSI 22;0 t`) at start and, on exit, writes an empty title
  and pops it (`CSI 23;0 t`). Ghostty discards the push and pop, and
  xterm.js has them off by default; there the empty title lets the
  terminal fall back to its own.

## Open questions

- Which terminals support OSC 9;4, and whether asqr only sends it where it
  is known to work. Any session text written into an OSC drops control
  characters first, so an agent-written title cannot end the sequence.
