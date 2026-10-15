---
title: "Delete Kitty images the terminal no longer shows"
kind: bug
component: tui
impact: high
origin: review
tags: [ux]
---
# Delete Kitty images the terminal no longer shows

Found in the adversarial review of the redesign on 2026-10-15 (findings
F2, O5 to O11, F4 to F10) and requested by the user. Under the Kitty
graphics protocol, every new encode of a picture is sent with a fresh
random image id (ratatui-image 11.1.0 `Picker::new_resize_protocol`,
`picker.rs:291-301`), and asqr never asks the terminal to delete the old
copy. Copies pile up in the terminal: the mascot on every size change
and every focus gain, context images on every focus gain, every replaced
entry and every session that leaves. Ghostty keeps them until its
320 MB limit; at Ghostty's cell size one copy of the mascot is about
1.7 MB. ADR 24 accepted this for context images ("asqr does not ask the
terminal to delete the earlier copies"); this todo replaces that.

## Decided design

- **A fresh id per encode, chosen by asqr.** asqr builds the Kitty
  protocol itself so it knows the id:
  `StatefulKitty::new(id, picker.tmux_detected(),
  picker.capabilities().contains(&Capability::KittyCompression))` inside
  `StatefulProtocol::new(picture, picker.font_size(), None, ..)`, with a
  comment that `None` mirrors asqr never setting a background colour.
  Getting `compress` wrong makes terminals refuse the transmission. The
  id is random, never 0, and drawn again if it is already live in this
  run. Other protocols keep `new_resize_protocol`. One constructor in
  `src/tui/render/images.rs` serves context images and the mascot.
- **Why not stable ids:** reusing an id makes Ghostty delete the old
  image when the new transmission begins
  (`graphics_exec.zig:1018-1026`), so every focus-gain re-send would
  blank the picture for the length of the transfer; and stable ids
  derived from session and entry would clash between two asqr instances
  sharing one terminal through tmux.
- **Every drop queues the old id.** An id lives with what holds it (the
  kept image viewer, the mascot's encoded picture). One private method
  puts it on a pending-delete list on every path that drops a holder:
  `Viewers::forget_images` (FocusGained), `Viewers::retain_sessions`, the
  replacement in `Viewers::laid_out` (image to image included, image to
  unreadable image, image to another kind), the mascot's re-encode for
  another picture, size or forget, and exit.
- **Retain on every draw.** `view.tab.retain(app.sessions())` moves
  before the `match app.active()` in `draw_screen` (and runs for a
  too-small frame too), so the last session leaving queues its deletes
  on the first empty-queue frame (`screen.rs:191-209` today retains only
  while a session shows).
- **When deletes are written.** The pending list is drained at the end
  of each draw into what the draw returns (`Drawn`). The run loop writes
  them after that frame is flushed, before the `continue` at
  `run.rs:109-111`, through a new `Host::delete_images(&[u32])`;
  `TerminalHost` writes through `io::stdout().lock()` and flushes, the
  test `Recorder` records them. In the frame that drops an id, every cell
  that showed it is rewritten (new placeholder colours or blanks), so the
  delete always follows the new transmission in the output.
- **The delete sequence.** Per id `ESC _G a=d,d=I,i=<id>,q=2 ESC \`.
  `d=I` with `i=` and no `p=` removes every placement, virtual ones
  included, and frees the data (Ghostty `graphics_storage.zig:1449-1478`,
  630-640); `q=2` keeps the terminal silent. Under tmux it is wrapped
  with `picker.tmux_detected()` and ratatui-image's public
  `Parser::tmux_start_escape_end`, the same flag and wrapping the
  transmissions use.
- **Repeat on focus gain.** The ids deleted since the last FocusGained
  are written once more after the next FocusGained frame, through the
  same path, then cleared. Under tmux with `allow-passthrough on` a
  delete written while the pane is hidden is lost; the repeat lands when
  the pane shows again, as ADR 24's re-send does. Deleting an unknown id
  is a no-op without a reply.
- **Exit.** Ghostty keeps Kitty images per screen, and the alternate
  screen persists after leaving it; a delete written after leaving does
  nothing, and erasing the display skips virtual placements. So one call
  on `View` drains all live ids (context images and the mascot) plus the
  pending list, written in `TerminalGuard::drop` before the title pop and
  `restore`, through a shared handle like the title's.
- **Scope.** ratatui-image 11.1.0 uses the Kitty protocol with kitty and
  Ghostty; it blacklists Kitty for WezTerm and Konsole
  (`picker.rs:118-128`), which get iTerm2 and are untouched.

## Tests

- One test per drop path, each showing the id on the pending list after
  the drop; a half-block or iTerm2 view queues nothing.
- The run loop finishes the only session and the `Recorder` sees the
  delete after the empty-queue frame.
- The delete bytes with and without tmux.
- asqr's Kitty protocol and `new_resize_protocol`'s transmit the same
  apart from `i=`.
- Invariant test (reviewer O): in a run-loop test with a Kitty picker,
  read the ids each frame shows from the buffer (the placeholder cells'
  RGB foreground plus the third diacritic) and assert that no id deleted
  after a frame shows in that frame or any later one, through
  FocusGained, a replaced entry, an image-to-image replacement, a mascot
  resize and a session leaving.
- The focus-gain repeat writes the earlier deletes once and then clears.

## Docs

ADR 24 gets an amendment (or a new ADR replacing its "does not delete"
consequence), ADR 39 notes the mascot's ids follow the same rule. Record
as consequences: under tmux the repeat depends on `focus-events on`;
exit deletes from a hidden pane are lost; a panic or a signal leaves
the copies until the terminal evicts them. The id rule and the exit
order follow Ghostty's source; for kitty itself, keeping images per
screen is a hypothesis.

The design and both review rounds are in the logbook
`~/playground/2026_10_15_asqr-maskottchen/99-plan-kitty-image-ids.md`
and `99-review-impl-ledger.md`.
