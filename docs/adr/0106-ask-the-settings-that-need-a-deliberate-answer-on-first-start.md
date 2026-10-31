# 106. Ask the settings that need a deliberate answer on first start

Date: 2026-10-31

## Status

Accepted

Amends [92. Keep settings in a file](0092-keep-settings-in-a-file.md):
the TUI asks the settings that need a deliberate answer when it starts with
no settings file

## Context

- A missing settings file gives every default and says nothing (spec
  section 5.2).
- `keep_files` decides whether asqr copies the files a finished session
  showed into its archive (ADR 100). Files a session shows are often
  temporary, and replay shows them later only if a copy was kept.
- The other settings (the diff layout, the question's width and where the
  context goes) have defaults a person can judge only after a session.

## Decision

- The first time the TUI starts with no settings file, asqr shows a
  first-start screen. It asks the settings that need a deliberate answer,
  writes the file and does not ask again. `keep_files` is the only setting
  it asks.
- The screen shows after the terminal is set up, before the start picker
  (ADR 86) and before the first read of the queue. The tmux pin (ADR 85) is
  set before it.
- The screen is the queue switcher's frame, titled `First start`, with
  `writes <path of settings.toml>` on its bottom edge. It holds the lead
  `Before your first session, one setting needs your answer. Press , later
  to change it.`, the question `Keep copies of the files a session showed
  you?`, a dim line `Files a session shows are often temporary. Replay can
  show them later only if a copy was kept.`, the three values of the pane's
  `Keep files` row with the pane's own texts, `compact copies` picked and
  marked `◀ default`, the button `[ Save and start ]` and the line `q quits
  without writing; the next start asks again.`
- `↑`/`↓` and `k`/`j` move, `enter` or `space` picks a value, `enter` on the
  button writes the file and starts asqr, and `q` and `ctrl-c` quit.
- Saving writes `keep_files` only, through the writer the pane uses, and
  shows `✔ saved keep files: <value>`. A write that fails is reported as the
  pane reports it, the start goes on with the chosen value for this run, and
  the next start asks again.
- Quitting writes nothing, so the next start asks again. Quitting is no
  answer.
- The screen shows only for `asqr` and `asqr watch`. `asqr replay` and the
  commands agents call never show it.
- A settings file that is there never triggers the screen, whether it is
  empty, lacks `keep_files`, cannot be read or does not parse. When
  `ASQR_KEEP_FILES` is set, the setting is decided: the screen does not
  show, nothing is written, and the pane shows the row locked.
- The update check, once it exists, joins the screen as a second question.
- Rejected: starting with the defaults and only naming the file; asking the
  settings of taste, which nobody can judge before a session; a one-time line
  in the status line instead of a screen; parking the screen until the update
  check exists; quitting writing the defaults.

## Consequences

- A person who always quits the screen sees it at every start.
