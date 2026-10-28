# 73. Report progress through OSC 9;4

Date: 2026-10-28

## Status

Accepted

Amends [40. Set the terminal title](0040-set-the-terminal-title.md):
progress through OSC 9;4 goes to the terminals known to show it

## Context

asqr runs in a terminal of its own, often in a tab the person does not
look at. The terminal title counts the waiting sessions (ADR 40), which
left reporting progress through OSC 9;4 open until it is known which
terminals support it.

OSC 9;4 is the progress sequence ConEmu introduced,
`ESC ] 9 ; 4 ; <state> ; <percent> ST`: state 1 sets the progress to the
percent, state 0 removes it. iTerm2 and kitty before 0.38 take an
`OSC 9 ; <text>` as a desktop notification, so a terminal that does not
know OSC 9;4 may show the sequence as one.

Which terminals show it, from their documentation, changelogs and
source:

| Terminal | Shows OSC 9;4 | Since | Known by |
|---|---|---|---|
| Ghostty | as a bar at the top of the split; it clears the bar after about 15 seconds without an update and asks for one at least every second | 1.2.0 | `TERM_PROGRAM=ghostty` with `TERM_PROGRAM_VERSION`; XTVERSION `ghostty <version>` |
| iTerm2 | as a bar in the session, from 3.7.0 also on the tab | 3.6.6; 3.6.6 fails on an OSC 9;4 without a state, fixed in 3.6.7 | `TERM_PROGRAM=iTerm.app` with `TERM_PROGRAM_VERSION`, or `LC_TERMINAL=iTerm2` with `LC_TERMINAL_VERSION`; XTVERSION `iTerm2 <version>` |
| Windows Terminal | as a ring on the tab and in the taskbar | 1.6 | `WT_SESSION`; it names no version |
| kitty | as a percentage in the tab's title | 0.39.0; 0.38.0 drops it, earlier versions show a notification | XTVERSION `kitty(<version>)`; its environment names no version |
| VTE (GNOME Terminal, Ptyxis) | as a terminal property; it takes no OSC 9;4 that ends with BEL | version unknown | `VTE_VERSION` |
| WezTerm | in its nightly builds only | | `TERM_PROGRAM=WezTerm` |
| tmux | from 3.7 it keeps the progress of a pane and can pass it on; before, it drops the sequence | 3.7 | `TMUX` |
| foot | ignores it | | `TERM=foot` |

Sources: the release notes of Ghostty 1.2.0 and its documentation of
the ConEmu sequences; the release notes of iTerm2 3.6.6, 3.6.7 and 3.7.0
and its escape code documentation; the kitty changelog (0.38.0, 0.39.0);
Microsoft's documentation of the progress sequences in Windows Terminal;
VTE's `src/vteseq.cc`; the tmux CHANGES file (3.6b to 3.7); WezTerm's
documentation of `pane:get_progress`; foot's `osc.c`.

## Decision

- **The value:** the share of the active session's questions that have
  an answer, in percent and rounded down, from the counts the review
  shows: an untouched default counts as answered, and info items count
  neither way. The review tab keeps the share, so it reads 100 % once
  every question is answered.
- **No progress:** while no session waits, and for a session of info
  items only.
- **The bytes:** `ESC ] 9 ; 4 ; 1 ; <percent> ESC \` sets the progress,
  `ESC ] 9 ; 4 ; 0 ESC \` removes it. They end with ST, as VTE takes no
  BEL there, and the remove names its state, as iTerm2 3.6.6 fails on an
  OSC 9;4 without one. Nothing from a session goes into them.
- **When:** the progress goes out when it changes, as the title does,
  and again after a focus gain, as tmux drops passthrough from a pane no
  client shows (ADR 24). A session that ends changes it to the next
  session's, or removes it.
- **Terminals:** Ghostty from 1.2.0, iTerm2 from 3.6.6, Windows Terminal
  and, inside tmux, kitty from 0.39.0. Every other terminal and a
  version that cannot be read get nothing, and so do GNU screen and
  Zellij, which draw their own screen and pass on the variables of the
  terminal they were started from.
- **tmux:** the terminal is the client's, known from its answer to
  XTVERSION, as for Talqi's glyphs (ADR 43), and only while one client
  is attached. A client can detach and another attach from a different
  terminal, so asqr asks tmux again before every write. The sequence
  goes out in the passthrough wrapper, as the notification does. Where
  the check fails, asqr writes nothing, a remove neither, and counts the
  progress as gone.
- **Keep-alive:** for Ghostty, asqr sends the progress again every
  second while one shows, from a thread that wakes the run loop, which
  writes it without drawing.
- **Quitting:** a progress asqr shows is removed when it quits, also
  after a panic, before it leaves the alternate screen.
- No flag switches the progress off.

## Consequences

- Ghostty and iTerm2 before 3.7.0 show the progress in the window, not
  on the tab.
- A Windows Terminal before 1.6 is not told apart and gets the progress.
- kitty outside tmux gets no progress.
- Inside tmux, asqr runs `tmux display-message` before every write of
  the progress, once a second while Ghostty shows one.
