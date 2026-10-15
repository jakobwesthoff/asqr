# 44. Pass Kitty images through tmux from a hidden pane

Date: 2026-10-16

## Status

Accepted

Amends [24. Resend images when asqr regains the focus](0024-resend-images-when-asqr-regains-the-focus.md)

Amends [42. Delete Kitty images the terminal no longer shows](0042-delete-kitty-images-the-terminal-no-longer-shows.md)

Amended by [45. Keep image handling in a graphics module around an asqr-owned value](0045-keep-image-handling-in-a-graphics-module-around-an-asqr-owned-value.md):
the focus rule of ADR 24 and the repeated deletes of ADR 42 hold for
Kitty images only, not for Sixel and iTerm2

## Context

- tmux with `allow-passthrough on` passes escape sequences only from a
  pane a client shows; with `all` it passes them from hidden panes too
  (ADR 24, reproduced with the user's configuration). Under `on`, a
  Kitty transfer or delete written while asqr's pane is hidden is lost,
  and asqr depends on focus reports to send images and deletes again.
- ratatui-image 12.0.0-rc.2 runs `tmux set -p allow-passthrough on` for
  the pane in every `Picker` constructor when `TERM` starts with `tmux`
  or `TERM_PROGRAM` is `tmux`, and asqr puts the pane's own value back
  afterwards (spec section 7.7).
- The Kitty protocol shows an image only where placeholder cells name
  it, and those are part of the pane's own content, which tmux draws
  only for a pane that shows. Sixel and iTerm2 images are written where
  the cursor is; tmux writes passthrough without positioning it, so from
  a hidden pane they would paint over the window that shows (review of
  2026-10-15).
- A query tmux does not pass on gets no answer and costs its whole wait.
- On 2026-10-15 the user decided that, under tmux 3.4 or later and the
  Kitty protocol, asqr sets `allow-passthrough all` for its pane instead
  of `on`, and restores the previous value on exit.

## Decision

- asqr asks tmux once at start, with the question it already asks for
  passthrough and attached clients, also whether the pane's window is
  the current one of its session, whether the pane is the current one
  of its window, whether the window is zoomed, and tmux's version.
- asqr queries the terminal only when a client shows the pane: its
  window is the session's current one, and no other pane of the window
  is zoomed. This holds where the pane has `all` too, since
  ratatui-image sets `on` before its query. Otherwise images show as
  blocks for the run.
- When the query found the Kitty protocol, ratatui-image wraps its
  commands for tmux, tmux is 3.4 or later and the pane does not have
  `all` already, asqr sets `allow-passthrough all` for its pane for the
  run. A development build counts by the version it names, as
  `next-3.6`; a version in any other form does not count.
- asqr puts the pane's value from before the query back when it quits,
  after it has written the deletes of its Kitty images, or unsets it
  when the pane had none of its own. Without that value, when tmux
  cannot show the option, asqr does not set `all`.
- Sixel and iTerm2 keep the pane's own setting.
- asqr changes no server option, such as `input-buffer-size`.

## Consequences

- Under the Kitty protocol inside tmux 3.4 or later, an image that
  arrives while asqr's window is in the background shows when the person
  switches to it, without focus reports, and the deletes asqr writes
  from a hidden pane, those on quitting included, reach the terminal.
- A SIGKILL or a panic that aborts leaves `all` on the pane. A panic
  that unwinds, SIGTERM, SIGINT and SIGHUP put the value back.
- Inside tmux before 3.4, and for Sixel and iTerm2, the focus rule of
  ADR 24 and the repeated deletes of ADR 42 stay what brings images and
  deletes through `on`.
- A pane no client shows at start gets its images as blocks until asqr
  restarts.
