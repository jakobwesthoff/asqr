# 110. Send Kitty images again after a tmux session switch

Date: 2026-11-16

## Status

Accepted

Amends [24. Resend images when asqr regains the focus](0024-resend-images-when-asqr-regains-the-focus.md):
under `allow-passthrough all` a focus gain sends the images again when
what asqr wrote since the last gain may have been lost

Amends [42. Delete Kitty images the terminal no longer shows](0042-delete-kitty-images-the-terminal-no-longer-shows.md):
under `all` the deletes do not pass while no client shows asqr's
session; a focus gain repeats them where they may have been lost

Amends [44. Pass Kitty images through tmux from a hidden pane](0044-pass-kitty-images-through-tmux-from-a-hidden-pane.md):
`all` passes from a hidden window of a session a client shows, not from
a session no client shows

Amends [45. Keep image handling in a graphics module around an asqr-owned value](0045-keep-image-handling-in-a-graphics-module-around-an-asqr-owned-value.md):
images are sent again on a focus gain under `all` too, by the rule
below, and such a resend reads the image files again

Amended by [111. Warn when tmux sends no focus reports](0111-warn-when-tmux-sends-no-focus-reports.md):
asqr warns at start where tmux sends no focus reports

## Context

An image of a session that arrived while the tmux client showed another
session stayed blank after switching back to asqr's session. Changing
tabs, switching tmux sessions again and taking the focus from the
terminal and back did not bring it; only asking the session again did.
Before 0.12, asqr sent the images again on every focus gain. ADR 45
limited that to `allow-passthrough on`, and under the Kitty protocol
inside tmux 3.4 or later asqr sets `all` for its pane (ADR 44), so no
focus gain sent anything again.

Measured with tmux 3.8 on a server of its own, with clients in
pseudo-terminals:

- With `all`, a passthrough sequence from asqr's pane reached a client
  that showed the pane's window or another window of the pane's
  session. It reached no client while the client showed another
  session, and none while no client was attached. With `on` it reached
  a client only while the pane's window was shown.
- The pane got `ESC [ O` on `switch-client` to another session, on
  `select-window` to another window, on `detach-client`, and when the
  client's terminal reported a focus loss; it got `ESC [ I` on the way
  back and when a client attached.
- A client process killed with SIGKILL sent no `ESC [ O`; the next
  client attaching sent `ESC [ I`.
- Turning focus reports on with `?1004h` got no report, with no client
  attached and with a focused client showing the pane.
- tmux passed a Kitty transmission of 300 KB in chunks of 4096 bytes to
  the client 1.4 ms after the write to the pane returned, 1.5 MB after
  9.5 ms and 4 MB after 24.9 ms.
- `#{?session_grouped,#{session_group_attached},#{session_attached}}`
  for the pane gave 0 with no client, 1 with the client on the pane's
  window, on another window of its session or on a session grouped with
  it, and 0 after the client switched to another session or detached.

## Decision

The rule applies to the Kitty protocol behind tmux with `all`. With
`on` every focus gain still sends the images again and repeats the
deletes (ADRs 24 and 42); without tmux, and under the other protocols,
no focus gain sends anything again.

asqr notes, for each frame and the deletes written after it, whether it
sent an image transmission and whether it wrote deletes. The key reader
stamps each focus report with the time it read it. What such a frame
wrote counts as possibly lost when:

- it was written while asqr had no focus and tmux says no client is
  attached to the session of asqr's pane or a session grouped with it,
  or tmux gives no number;
- it is the last such frame, and a focus loss arrived before it was
  done or less than one second after;
- it is the last such frame, it was written while asqr had no focus,
  and the focus gain arrived less than one second after it was done,
  whatever tmux said about it;
- a focus gain arrived with no focus loss before it, for everything
  written since the previous gain.

On a focus gain, asqr sends again every image a frame showed and repeats
the deletes since the previous gain when a transmission may have been
lost, repeats only the deletes when only deletes may have been lost,
and otherwise forgets the deletes since the previous gain. The deletes
written in the frame after a repeat are kept for the next gain that
repeats.

asqr asks tmux with `display-message` only after a frame that wrote
Kitty commands while it had no focus, and not again for a kind of
command already counted as possibly lost before the next gain. tmux gets
500 ms to answer. A failed call is logged.

Rejected:

- Sending the images again on every focus gain under `all`, as before
  0.12.
- Reading the event channel again after each frame to see a focus loss
  that arrived meanwhile. It does not cover a frame still in the pane's
  output when tmux hid the pane, and a load event taken from the channel
  there would cost an extra draw.

## Consequences

- An image or a delete written while the tmux client showed another
  session, was detached or was killed goes out again on the next focus
  gain. With `focus-events off` tmux sends no focus reports, and nothing
  goes out again.
- An image written while the client showed another window of asqr's
  session, or while the terminal had lost the focus to another app, is
  not sent again.
- A focus loss within one second of a frame that wrote Kitty commands,
  and the first focus gain with no loss before it after such a frame,
  send the images again although they may have arrived. A window linked
  into another session counts as not shown for that session's clients.
- Sending an image again reads its file again, so a file changed or
  removed since shows as it is now. ADR 45 had a changed file keep
  showing what asqr read first under `all`.
- The run loop waits for tmux after a frame that wrote Kitty commands
  while asqr had no focus.
- The deletes asqr writes on quitting while no client shows its session
  do not reach the terminal.
- asqr starts as having the focus.
