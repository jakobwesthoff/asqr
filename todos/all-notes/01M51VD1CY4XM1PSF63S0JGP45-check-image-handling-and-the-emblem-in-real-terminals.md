---
title: "Check image handling and the emblem in real terminals"
kind: chore
component: tui
origin: implementation
tags: [ux]
---
# Check image handling and the emblem in real terminals

The image handling and the empty-queue emblem were rebuilt on 2026-10-15
and 2026-10-16 (ADRs 42 to 46: Kitty deletes, ratatui-image
12.0.0-rc.2 with the late-answer drain and `allow-passthrough all`,
the graphics module, the emblem in text cells). The tests drive asqr
through byte-exact checks, pipe tests and a PTY with stand-in answers,
but no real terminal. The user postponed the check in real terminals
and asked to assume for now that it works.

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
- A pane hidden at start (another window, or zoomed away) gets blocks
  without the 5 s wait.
- Quitting and a panic leave no images in the terminal (Ghostty's
  memory stays flat when sessions with images are opened and closed
  repeatedly).
- WezTerm gets the iTerm2 protocol, Konsole blocks.

The same list is in the logbook
`~/playground/2026_10_15_asqr-maskottchen/99-manuelle-pruefung.md`.
