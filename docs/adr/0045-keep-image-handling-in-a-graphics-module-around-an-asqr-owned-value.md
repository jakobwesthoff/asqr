# 45. Keep image handling in a graphics module around an asqr-owned value

Date: 2026-10-16

## Status

Accepted

Amends [24. Resend images when asqr regains the focus](0024-resend-images-when-asqr-regains-the-focus.md)

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amends [42. Delete Kitty images the terminal no longer shows](0042-delete-kitty-images-the-terminal-no-longer-shows.md)

Amends [44. Pass Kitty images through tmux from a hidden pane](0044-pass-kitty-images-through-tmux-from-a-hidden-pane.md)

Amended by [46. Build and lay out context viewers on worker threads](0046-build-and-lay-out-context-viewers-on-worker-threads.md):
an image is decoded and encoded apart from drawing, and keeps its file
as read instead of its decoded picture

Amended by [48. Mend Kitty images an overlay is drawn over](0048-mend-kitty-images-an-overlay-is-drawn-over.md):
the screen mends a Kitty image once the overlays are drawn

## Context

- The library took the detected image protocol as ratatui-image's
  `Picker`. The picker was handed through the view, the tab, the viewers
  and the encoder, and about sixty tests built one with
  `Picker::halfblocks()`.
- Every `Picker` constructor in ratatui-image 12.0.0-rc.2 runs
  `tmux set -p allow-passthrough on` when `TERM` starts with `tmux` or
  `TERM_PROGRAM` is `tmux`. A test run inside tmux set the test's own
  pane to `on`, also where the person had `all`: with a stand-in `tmux`
  that notes its calls and `TERM=tmux-256color`, the library's tests
  called `tmux set -p allow-passthrough on` 272 times.
- The image code lived in one file of the screen's module, next to
  terminal detection, while the other half of detection, the snapshot
  and restore of the pane's `allow-passthrough`, lived in the binary.
- The screen was cleared after an overlay closed over an image under
  every protocol, although only Sixel and iTerm2 need it (ADR 31), and
  the images were read and sent again on every focus gain, although
  only Kitty transfers that tmux with `on` drops need it (ADR 24).
- A module of its own around an asqr-owned value lets both policies key
  on the protocol and the passthrough.

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
- The wrapping follows what ratatui-image sees: it wraps where the
  terminal names tmux. The passthrough follows what tmux does with the
  pane: `all` where asqr set it or found it, `on` where tmux reports
  `on`, none outside tmux. A terminal that names tmux outside a tmux
  pane, as after ssh from one, counts as `on`, since that tmux cannot be
  asked.
- Two policies key on these facts: the screen is cleared after an
  overlay over an image only where the protocol leaves skipped cells
  (Sixel, iTerm2; ADR 31), and images are sent again on regaining the
  focus only under the Kitty protocol with `on` (ADR 24).
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
- A test run runs no `tmux` command, also in a terminal that names
  tmux, and the `just test` and `just coverage` recipes set no `TERM` of
  their own. Checked with a stand-in `tmux` that notes its calls first
  on the path and `TERM=tmux-256color`: the whole test run calls it 0
  times. The comparison with the picker checks nothing in such a
  terminal.
- An image is decoded when its viewer is built and encoded for the
  body's size when the viewer is laid out, once per size, both on a
  worker (ADR 46), with ratatui-image's fixed-size protocols. A failed
  encoding drops the decoded picture.
- Under half blocks and the Kitty protocol, closing an overlay over an
  image does not redraw the whole screen. Outside tmux with `on`,
  regaining the focus does not read the images again or write the
  deletes since the last focus gain again (ADR 42). An image file
  changed on disk keeps showing what asqr read first, as it does
  between focus changes.
