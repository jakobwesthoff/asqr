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

## Amendment (2026-09-27, adversarial review F8, F9)

- **Text fields:** while one is open, every key goes to the editor
  except `esc`, which keeps the text.
- **Confirmations:** `S` and `X` always ask first and show the counts.
- **Quitting:** `ctrl-c` quits and keeps the draft, like `q`.
- **Session list:** `L` opens it.
- **Length counter:** shows text (`112/125`, `140/125 !`, `max`). Colour
  is only added on top.
- **Image protocol:** detected only in `main.rs`, and the library takes
  the result as a `Picker`.
- **Broken image paths:** relative, missing or unreadable paths show a
  placeholder.
- **tmux:** needs `allow-passthrough on` for images and notifications.
  This is a known limitation, and a hint for it is a todo.
