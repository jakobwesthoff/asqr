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
- Send both through the same path as the notifications in `alert.rs`,
  including its tmux passthrough.

## Decisions

- 2026-10-15, user: the title comes first, as `asqr · {n} waiting`, and a
  `--no-title` flag switches it off, like `--no-notify` and `--no-bell`.
  Progress through OSC 9;4 follows only after checking which terminals
  support it.

## Open questions

- Which terminals support OSC 9;4, and whether asqr only sends it where it
  is known to work.
- Restoring the previous title when asqr quits.
- Whether tmux passes OSC 2 on by itself or needs the passthrough wrapper.
