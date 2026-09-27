# 8. TUI interaction model

Date: 2026-09-27

## Status

Accepted

## Context

The person answering has to get through many questions fast, often with
long option texts and sometimes with an image.

## Decision

- **Layout:** the question list with each question's state on the left,
  and the current question on the right.
- **Keys:** vim keys plus arrows.
  - `j`/`k` and the arrow keys move within the options.
  - `tab`, `shift-tab`, `J` and `K` move between questions.
  - `space` or `enter` selects, and `1`-`9` picks an option directly.
  - `c` opens the custom entry and `n` the note.
  - `S` submits and `q` quits while keeping the draft.
  - `?` shows the help.
- **Drafts:** answers are saved continuously, so nothing is lost on quit
  or crash.
- **New sessions:** arrive with a desktop notification (OSC 9 or 777) and
  a terminal bell. Both can be switched off.
- **Images:** optional per question. A column to the right when the
  terminal is wide enough, below the options otherwise. `z` shows the
  image full screen and `o` opens the system viewer.
- **Later, not in version 1:** `asqr answer <file>`, which answers one
  file in the current terminal without a queue.

## Consequences

The key map is fixed in version 1. A configurable key map can come later
without breaking the file format.
