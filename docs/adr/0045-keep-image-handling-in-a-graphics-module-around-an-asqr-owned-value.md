# 45. Keep image handling in a graphics module around an asqr-owned value

Date: 2026-10-16

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md)

## Context

- ADR 8 has the library take the detected image protocol as
  ratatui-image's `Picker`. The picker was handed through the view, the
  tab, the viewers and the encoder, and about sixty tests built one with
  `Picker::halfblocks()`.
- Every `Picker` constructor in ratatui-image 12.0.0-rc.2 runs
  `tmux set -p allow-passthrough on` when `TERM` starts with `tmux` or
  `TERM_PROGRAM` is `tmux`. A test run inside tmux set the test's own
  pane to `on`, also where the person had `all`. The `just test` recipe
  set a plain `TERM` and an empty `TERM_PROGRAM` to avoid it.
- The image code lived in one file of the screen's module, next to
  terminal detection, while the other half of detection, the snapshot
  and restore of the pane's `allow-passthrough`, lived in the binary.
- The architecture review of 2026-10-16 proposed a module of its own
  around an asqr-owned value; the lead decided to do it after the move
  to 12.0.0-rc.2.

## Decision

- A library module `graphics` holds the image code: the detected facts,
  the images of context entries, the Kitty id book and the deletes, and
  the decisions taken at start (whether to query, the pane's
  passthrough, the late answers).
- `Graphics` is a plain copyable value of what asqr learnt at start: the
  protocol, the cell size, whether commands go out wrapped for tmux, the
  passthrough that holds for the run, and whether the terminal inflates
  compressed Kitty transmissions. The binary builds it once from the
  picker, or takes half blocks without one.
- `Images` makes the images for a `Graphics` and holds the one Kitty id
  book of the run. The binary makes it once and hands it to the view.
- Tests build `Graphics` and `Images` directly. One test compares
  asqr's protocols with what the picker builds; it builds a `Picker`
  only where the terminal does not name tmux.
- What the alerts, the emblem's glyphs and the graphics read from the
  environment and from tmux is read once, in a module beside `graphics`
  that all three use.

## Consequences

- Outside `graphics`, the binary's start is the only code that sees
  ratatui-image's picker.
