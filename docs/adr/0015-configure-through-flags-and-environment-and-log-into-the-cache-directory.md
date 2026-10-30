# 15. Configure through flags and environment and log into the cache directory

Date: 2026-10-16

## Status

Accepted

Amended by [56. Show text with ANSI colours as a context entry](0056-show-text-with-ansi-colours-as-a-context-entry.md)

Amended by [83. Set which events reach the log through ASQR_LOG](0083-set-which-events-reach-the-log-through-asqr-log.md)

Amended by [90. Keep what the person typed out of the log](0090-keep-what-the-person-typed-out-of-the-log.md)

## Context

asqr has few settings: the queue, notifications and the bell. The
TUI owns the terminal, so logs cannot go to stdout or stderr.

crossterm 0.29 reads `NO_COLOR` too, and then writes every colour as an
empty SGR sequence, which a terminal reads as a reset of all attributes.
RGB colours are lost with it, Kitty images with their id, and a cell
that changes colour loses the bold, dim or reversed written just before
its colour.

## Decision

- Settings come from command-line flags and environment variables only,
  for example `--no-bell`, `--no-notify`, `--queue` and `ASQR_QUEUE`.
  There is no config file.
- The log goes to a file in the platform cache directory (`directories`),
  and `asqr paths` shows where.
- asqr reads `NO_COLOR` (<https://no-color.org>). It counts when it is
  present and not empty, whatever its value, a value that is not UTF-8
  included. An empty `NO_COLOR` counts as unset, as for asqr's own
  variables.
- Under it, the TUI resets every named and indexed colour of the drawn
  screen in one pass after drawing, and keeps every modifier. Marks,
  pointers, text, bold, dim and reversed carry the meaning, as ADR 8 and
  the design guide ask.
- The pass works on the finished screen rather than on each style, so it
  covers every colour: those of the colour roles, those patine gives code
  and Markdown, and any colour a later change draws without a role.
- RGB colours stay. Only images and Talqi draw with them: half blocks
  paint the picture in RGB, and the Kitty protocol names each image by
  the RGB foreground of its placeholder cells. Images keep their colours
  under `NO_COLOR`. Talqi is an outline without any colour under
  `NO_COLOR` (ADR 43).
- A Markdown quote's bar is the one style chosen before drawing: under
  `NO_COLOR` the screen asks patine for a dim bar instead of a blue one,
  as the pass would leave a blue bar in the text colour (ADR 35).
- asqr forces crossterm's colour output on, so the drawn screen alone
  decides what shows.

Rejected: a TOML config file from the start. For the log: the queue root,
and logging only when asked with `--log`.

## Consequences

A config file can come later without changing the flags. Logs are
disposable, as a cache directory implies.

A screen without an image has no RGB colour, apart from Talqi's cells
on the empty queue, and a test pins that. A
colour drawn in RGB outside an image would keep its colour under
`NO_COLOR`. A `ratatui-image` release that names Kitty images any other
way has to be checked against the pass.
