# 42. Delete Kitty images the terminal no longer shows

Date: 2026-10-16

## Status

Accepted

Amends [24. Resend images when asqr regains the focus](0024-resend-images-when-asqr-regains-the-focus.md):
asqr deletes the earlier copies of the images it sends again

Amends [40. Set the terminal title](0040-set-the-terminal-title.md):
on a panic, the title is given back before asqr leaves the alternate
screen

Amended by [44. Pass Kitty images through tmux from a hidden pane](0044-pass-kitty-images-through-tmux-from-a-hidden-pane.md):
inside tmux 3.4 or later, the deletes on quitting from a hidden pane
reach the terminal

Amended by [45. Keep image handling in a graphics module around an asqr-owned value](0045-keep-image-handling-in-a-graphics-module-around-an-asqr-owned-value.md):
the deletes since the last focus gain are written again only where the
images are sent again on regaining the focus

## Context

Copies of images pile up in the terminal under the Kitty graphics
protocol unless asqr deletes them.

- A context image is encoded again for a new size, for a replaced entry
  and on a focus gain where images are sent again (ADR 24), and the
  images of every session that leaves are let go. Without deletes,
  Ghostty keeps every copy until its limit of 320 MB.
- ratatui-image gives every image it encodes for the Kitty protocol a
  new random id, and has no command that deletes one.
- ratatui-image uses the Kitty protocol in kitty and Ghostty. It rules
  the protocol out where `WEZTERM_EXECUTABLE` or `KONSOLE_VERSION` is
  set, and WezTerm and Konsole get the iTerm2 protocol.
- Ghostty deletes an image when a new transmission under the same id
  begins, so the picture is blank until the transfer ends.
- Ghostty keeps Kitty images per screen, and keeps the alternate screen
  with its images after a program leaves it. A delete written after
  leaving does nothing there, and erasing the display leaves images
  placed through placeholders, which ratatui-image uses.

The points on Ghostty come from its source.

## Decision

Ids:

- asqr builds the Kitty protocol of a context image itself, so it knows
  the image's id. It passes what ratatui-image's picker passes, the
  tmux wrapping and compression only where the terminal reported that
  it inflates it, and no background colour, which asqr never sets.
  Every other protocol comes from the picker.
- Every encoding gets a new random id. asqr draws again while the id is
  0 or one it handed out before in the same run.
- asqr does not reuse an id for a new encoding of the same image. With
  the same id Ghostty would blank the picture on every focus gain for
  the length of the transfer, and ids derived from the session and the
  entry would clash between two asqr processes sharing one terminal
  through tmux.
- A context image laid out at another size in cells gets a new id too:
  asqr builds a new protocol under a new id, from the bytes it decodes
  again for that size (ADR 46), and hands the old id on for deleting
  after the frame.

Deletes:

- Every way a context image goes hands its id on for deletion: the focus
  gain that drops the images to send them again, a session that no
  longer waits, an entry replaced by another image, by an image that
  cannot be read or by another kind of entry, a new size, and a viewer
  dropped outside the window of three questions (ADR 46).
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
- Where images are sent again on regaining the focus (ADR 45), the
  deletes written since the last focus gain are written once more after
  the frame that follows the next focus gain, together with that frame's
  own, and then forgotten. Inside tmux with `allow-passthrough on`, a
  delete written while asqr's pane is hidden is lost, as a transfer is
  (ADR 24); the repeat reaches the terminal once the pane shows again.
  Under `all` (ADR 44) the deletes pass from a hidden pane. A delete of
  an id the terminal does not know does nothing and gets no answer.
- One id book per run holds the Kitty ids, the deletes not yet written,
  the repeat after a focus gain and whether deletes go out wrapped for
  tmux, taken from the graphics the transmissions use. The book owns the
  deletes: they leave it once the frame is flushed. A frame that fails
  to flush ends the run, and its deletes are written on quitting. A
  delete that could not be written goes back into the book and is
  written after the next frame, or on quitting.
- On quitting, the view goes first, and every image hands its id on as
  it goes. A guard dropped next writes the deletes of every id left in
  the book while the alternate screen is on, then the terminal is given
  back, and last the pane's passthrough (ADR 44). The same holds when
  the run ends in an error.
- On a panic, asqr's panic hook runs before the one ratatui installs,
  which leaves the alternate screen. It writes the deletes of every id
  in the book, those of the images still shown included, gives the
  title back (ADR 40) and turns focus reports and bracketed paste off,
  then ratatui's hook runs. The pane's passthrough is given back as the
  panic unwinds, after both hooks, so the deletes pass while the pane
  has `all` (ADR 44). The hook only tries the book: after a panic while
  the book was locked, the deletes are written once the alternate screen
  is left already.

Talqi on the empty queue is drawn in text cells and sends no image
(ADR 43), so it leaves no copies to delete.

## Consequences

- Only kitty and Ghostty receive deletes, as only they get the Kitty
  protocol.
- Inside tmux with `allow-passthrough on`, the repeat depends on focus
  reports, which tmux forwards only with `focus-events on`. Without
  `all`, the deletes on quitting from a hidden pane are lost.
- A SIGKILL, and a panic in a build that aborts, end asqr without the
  deletes; the copies stay until the terminal evicts them. On Unix,
  SIGTERM, SIGINT and SIGHUP end the run as `q` does, and the deletes
  are written.
- ratatui-image does not hand out the picture it keeps, so the image
  keeps its file's bytes to decode them again for a new size (ADR 46).
- The rule on ids and the order on quitting follow Ghostty. That kitty
  keeps images per screen as well is not checked.
