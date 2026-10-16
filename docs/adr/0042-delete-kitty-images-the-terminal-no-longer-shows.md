# 42. Delete Kitty images the terminal no longer shows

Date: 2026-10-15

## Status

Accepted

Supersedes the consequence "asqr does not ask the terminal to delete the
earlier copies" of [24. Resend images when asqr regains the focus](0024-resend-images-when-asqr-regains-the-focus.md)

Amended by [44. Pass Kitty images through tmux from a hidden pane](0044-pass-kitty-images-through-tmux-from-a-hidden-pane.md):
inside tmux 3.4 or later, the deletes on quitting from a hidden pane
reach the terminal

Amended by [45. Keep image handling in a graphics module around an asqr-owned value](0045-keep-image-handling-in-a-graphics-module-around-an-asqr-owned-value.md):
the deletes since the last focus gain are written again only where the
images are sent again on regaining the focus

## Context

The adversarial review of the redesign on 2026-10-15 found that copies
of images pile up in the terminal under the Kitty graphics protocol, and
the user asked for them to be deleted.

- ratatui-image 11.1.0 gives every image it encodes for the Kitty
  protocol a new random id, and has no command that deletes one.
- asqr encoded a context image again on every focus gain (ADR 24) and
  for every replaced entry, and let the images of every session that
  left go. It never asked the terminal to delete the copies, as ADR 24
  accepted. Ghostty keeps them until its limit of 320 MB.
- ratatui-image uses the Kitty protocol in kitty and Ghostty. It rules
  the protocol out where `WEZTERM_EXECUTABLE` or `KONSOLE_VERSION` is
  set, and WezTerm and Konsole get the iTerm2 protocol.
- Ghostty deletes an image when a new transmission under the same id
  begins, so the picture is blank until the transfer ends.
- Ghostty keeps Kitty images per screen, and keeps the alternate screen
  with its images after a program leaves it. A delete written after
  leaving does nothing there, and erasing the display leaves images
  placed through placeholders, which ratatui-image uses.

The points on Ghostty come from its source, as the review read it.

## Decision

Ids:

- asqr builds the Kitty protocol of a context image itself, so it knows
  the image's id. It passes what ratatui-image's picker passes, the
  tmux wrapping and compression only where the terminal reported that
  it inflates it, and no background colour, which asqr never sets.
  Every other protocol comes from the picker as before.
- Every encoding gets a new random id. asqr draws again while the id is
  0 or one it handed out before in the same run.
- asqr does not reuse an id for a new encoding of the same image. With
  the same id Ghostty would blank the picture on every focus gain for
  the length of the transfer, and ids derived from the session and the
  entry would clash between two asqr processes sharing one terminal
  through tmux.

Deletes:

- Every way a context image goes hands its id on for deletion: the focus
  gain that drops the images to send them again, a session that no
  longer waits, and an entry replaced by another image, by an image
  that cannot be read or by another kind of entry.
- The viewers of sessions that left go on every draw, also one that
  shows the empty queue or only says the terminal is too small, so the
  last session to leave hands its ids on with the frame that no longer
  shows it.
- asqr writes the deletes after the frame that no longer shows the
  images is on the screen, one command per id:
  `ESC _G a=d,d=I,i=<id>,q=2 ESC \`. `d=I` with an id and no placement
  id removes every placement of the image, the virtual one included,
  and frees its data; `q=2` keeps the terminal from answering. Inside
  tmux the command is wrapped for passthrough as ratatui-image wraps
  its transmissions.
- The deletes written since the last focus gain are written once more
  after the frame that follows the next focus gain, together with that
  frame's own, and then forgotten. Inside tmux with
  `allow-passthrough on`, a delete written while asqr's pane is hidden
  is lost, as a transfer is (ADR 24); the repeat reaches the terminal
  once the pane shows again. A delete of an id the terminal does not
  know does nothing and gets no answer.
- When asqr quits, it deletes every image of a context entry it still
  knows of, and those handed on since the last frame, before it gives
  the title back and leaves the alternate screen. The same holds when
  the run ends in an error.

Talqi on the empty queue is left out. Its picture went through
ratatui-image with its own ids. ADR 43 draws Talqi in text cells, which
sends no image.

## Consequences

- Only kitty and Ghostty receive deletes, as only they get the Kitty
  protocol.
- A context image laid out at another size is encoded again by
  ratatui-image under the id asqr gave it. The terminal replaces the
  old copy, and Ghostty shows the image blank during the transfer.
- Inside tmux with `allow-passthrough on`, the repeat depends on focus
  reports, which tmux forwards only with `focus-events on`. The deletes
  on quitting from a hidden pane are lost.
- A panic or SIGKILL ends asqr without the deletes on quitting; the
  copies stay until the terminal evicts them. On Unix, SIGTERM, SIGINT
  and SIGHUP end the run as `q` does, and the deletes are written.
- Until ADR 43, Talqi's picture was sent again on every focus gain and
  every new size under the Kitty protocol, and its copies were never
  deleted. Talqi sends no image since.
- The rule on ids and the order on quitting follow Ghostty. That kitty
  keeps images per screen as well is not checked.

## Amendment (2026-10-16, ratatui-image 12.0.0-rc.2)

A context image laid out at another size gets a new id as well. Before
ratatui-image encodes it again, asqr builds a new protocol under a new
id from the decoded picture and hands the old id on for deleting after
the frame. This replaces the consequence that such an image is encoded
again under the id asqr gave it, which Ghostty showed blank during the
transfer. ratatui-image does not hand out the picture it keeps, so asqr
keeps a second decoded copy of each image under the Kitty protocol.

## Amendment (2026-10-16, the id book owns the deletes)

The architecture review of 2026-10-16 found that the deletes of a frame
left the id book before the frame was flushed, so a frame or a write
that failed lost them, and the lead decided to let the book own the
deletes.

- One book per run holds the Kitty ids, the deletes not yet written,
  the repeat after a focus gain and whether deletes go out wrapped for
  tmux, taken from the graphics the transmissions use.
- The deletes leave the book once the frame is flushed. A frame that
  fails to flush ends the run, and its deletes are written on quitting.
  A delete that could not be written goes back into the book and is
  written after the next frame, or on quitting.
- The book keeps the deletes since the last focus gain only where the
  images are sent again on regaining the focus (ADR 45).
