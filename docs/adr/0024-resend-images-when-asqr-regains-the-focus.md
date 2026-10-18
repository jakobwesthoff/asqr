# 24. Resend images when asqr regains the focus

Date: 2026-09-28

## Status

Accepted

Amended by [42. Delete Kitty images the terminal no longer shows](0042-delete-kitty-images-the-terminal-no-longer-shows.md):
asqr deletes the earlier copies of the images it sends again

Amended by [44. Pass Kitty images through tmux from a hidden pane](0044-pass-kitty-images-through-tmux-from-a-hidden-pane.md):
under the Kitty protocol inside tmux 3.4 or later, asqr sets
`allow-passthrough all` for its pane while it runs

Amended by [45. Keep image handling in a graphics module around an asqr-owned value](0045-keep-image-handling-in-a-graphics-module-around-an-asqr-owned-value.md):
asqr sends the images again on regaining the focus only under the Kitty
protocol behind tmux with `allow-passthrough on`, or behind a tmux it
cannot ask

Amended by [46. Build and lay out context viewers on worker threads](0046-build-and-lay-out-context-viewers-on-worker-threads.md):
asqr forgets only the images a frame showed and loads them again behind
the placeholder; an image loaded ahead and never shown is kept

## Context

The image of a session's first question never appeared when the session
arrived while asqr ran inside tmux in a window that was not shown.
Moving to another question and back did not bring it either.

In a separate tmux server attached from Ghostty:

- tmux with `allow-passthrough on` passes escape sequences only while
  the pane is visible (tmux(1)). With `all` it passes them from hidden
  panes too, and the image appeared.
- ratatui-image sends a Kitty image's data once, on the image's first
  draw, and afterwards only draws cells that refer to it by its id. A
  transfer tmux dropped is never repeated.
- tmux forwards focus reports to the pane when the person switches to
  its window, and so does the terminal when it gets the focus back.

## Decision

- asqr asks the terminal for focus reports. When its terminal gains the
  focus, and the images go out under the Kitty protocol behind tmux with
  `allow-passthrough on` or behind a tmux asqr cannot ask (ADR 45), asqr
  forgets the images a frame showed and loads them again behind the
  placeholder (ADR 46), so each one is read and sent again. An image
  loaded ahead and never shown has sent nothing and stays.
- README and spec recommend `set -g allow-passthrough all`, and say
  that with `on` asqr depends on focus reports, which tmux only forwards
  with `focus-events on`. Under the Kitty protocol inside tmux 3.4 or
  later, asqr sets `all` for its own pane (ADR 44).

## Consequences

- Inside tmux with `on` and `focus-events on`, an image that arrived in a
  hidden window appears when the person switches to it. With
  `focus-events off` it stays empty until asqr restarts; `all` avoids
  the problem entirely.
- Every focus change where the rule applies reads and sends the images
  shown again, including those that were fine. asqr deletes the earlier
  copies (ADR 42).
- A switch done by a tmux command while the terminal itself has no focus
  sends no focus report; the image appears once the terminal gets the
  focus.
