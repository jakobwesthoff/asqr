---
title: "Check image handling and the emblem in real terminals"
kind: chore
component: tui
status: blocked
origin: implementation
tags: [ux, field-test]
---
# Check image handling and the emblem in real terminals

The image handling and the empty-queue emblem were rebuilt on 2026-10-15
and 2026-10-16 (ADRs 42 to 46: Kitty deletes, ratatui-image
12.0.0-rc.2 with the late-answer drain and `allow-passthrough all`,
the graphics module, the emblem in text cells). The tests drive asqr
through byte-exact checks, pipe tests and a PTY with stand-in answers,
but no real terminal. The user postponed the check in real terminals
and asked to assume for now that it works.

asqr moved to ratatui-image 12.0.0-rc.3 on 2026-10-19 after the
Ghostty check below. rc.3 changes only the graphics query: it writes
the query straight to the stdout descriptor with `rustix::io::write`
and reads the answers from a borrowed stdin descriptor, where rc.2 went
through `io::stdout()` and `io::stdin()`. Every terminal checked below
before that date ran rc.2, so the query rows ("No digits or keys leak",
the detected protocol) count for rc.3 only once checked again.

## Checked so far

Ghostty under tmux, release build of `9b22439`, 2026-10-16, by Jakob
through a test set of seven asqr sessions: no key or digit arrived at
start; seven of ten image formats showed right, and PNG, JPEG and PPM
looked wrong only through faults in the test files (holes cut into the
PNG, no alpha channel in JPEG and PPM); scaling, `enlarge: false`,
extreme aspect ratios and a 6000×6000 image showed right; `z`, `o`, resizing, a
font-size change, switching tmux windows and zooming another pane
worked; no image stayed behind. Overlays over a Kitty image were broken
and are mended by ADR 48, which is not yet checked in a real terminal.
Still open in Ghostty: detach and attach, a second
client, quitting and restarting, the pane's `allow-passthrough` after
quitting, an image arriving in a background window, and the emblem.

## What to check

Record per row the protocol asqr detected, from
`~/Library/Caches/asqr/asqr.log` ("graphics protocol detected", "no
graphics protocol detected" with its reason, "discarded late
answers", "allow-passthrough all").

Terminals: Ghostty, kitty, WezTerm (also over ssh), iTerm2 (3.6.7 or
later for native sextants, 3.5 or later for Kitty), foot or
`xterm -ti vt340`, Konsole, VS Code, Windows Terminal, Terminal.app (the
quadrant fallback), and `NO_COLOR` with Kitty. Each without tmux and in
tmux 3.4 or later (pane `on` and `all`), plus tmux with two clients
attached, and a tmux before 3.4 if one is at hand.

- Empty queue: the emblem in sextants where recognised, quadrants in
  Terminal.app; colour without a rim, under `NO_COLOR` the outline
  without colour sequences; shown from 32 rows, below only the sentence.
- Context images fill exactly their cells, also after a font-size
  change and in a second tmux client with another cell size.
- No digits or keys leak from the query answers at start (no option
  gets picked by itself).
- Under tmux 3.4 or later with Kitty the pane gets `all` and gets its
  own value back after `q`, SIGTERM and a panic
  (`tmux show-options -p allow-passthrough`).
- An image that arrives in a background window shows on switching to
  it, also without `focus-events`.
- Resizing the pane in Ghostty does not blank the image; the old id's
  delete is written.
- The help, the intro and the session list over a Kitty image in the
  pane, and the help over it with `z`, leave the image's cells beside
  them in place, in Ghostty and kitty, without tmux and in tmux. An
  image first drawn under an overlay (a resize with the help open;
  under tmux `on`, regaining the focus with the help open) shows beside
  it at once and whole once it closes.
- A pane hidden at start (another window, or zoomed away) gets blocks
  without the 5 s wait.
- Quitting and a panic leave no images in the terminal (Ghostty's
  memory stays flat when sessions with images are opened and closed
  repeatedly).
- WezTerm gets the iTerm2 protocol, Konsole blocks.

The same list is in the logbook
`~/playground/2026_10_15_asqr-maskottchen/99-manuelle-pruefung.md`.

## Decision (Jakob, 2026-10-31, in the chat)

The checks wait for the final ratatui-image 12.0.0, as this todo says;
asqr pins 12.0.0-rc.3. Rejected: running them on rc.3 now and repeating
only the query rows later. Until then the overlay dimming (ADR 84) and
the replay (ADR 101) stay unchecked in real terminals. The other
questions prepared on 2026-10-31 (the emblem in quadrant-only terminals
such as Terminal.app, images under an overlay) come with the results.

## Relations

- Depends on: [Move ratatui-image from 12.0.0-rc.3 to its final release](01M6217WNNH2P5GF1545D08C2Z-move-ratatui-image-from-12-0-0-rc-3-to-its-final-release.md)
