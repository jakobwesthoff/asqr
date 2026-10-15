# 15. Configure through flags and environment and log into the cache directory

Date: 2026-09-27

## Status

Accepted

Mascot's colours under `NO_COLOR` superseded by [41. Show Talqi as a braille outline under NO_COLOR](0041-show-talqi-as-a-braille-outline-under-no-color.md)

## Context

Version 1 has few settings: the queue, notifications and the bell. The
TUI owns the terminal, so logs cannot go to stdout or stderr.

## Decision

- Settings come from command-line flags and environment variables only,
  for example `--no-bell`, `--no-notify`, `--queue` and `ASQR_QUEUE`.
  There is no config file in version 1.
- The log goes to a file in the platform cache directory (`directories`),
  and `asqr paths` shows where.

Rejected: a TOML config file from the start. For the log: the queue root,
and logging only when asked with `--log`.

## Consequences

A config file can come later without changing the flags. Logs are
disposable, as a cache directory implies.

## Amendment (2026-10-15, NO_COLOR)

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
- RGB colours stay. Only images draw with them: half blocks paint the
  picture in RGB, and the Kitty protocol of `ratatui-image` 11.1.0 names
  each image by the RGB foreground of its placeholder cells. Images, the
  mascot included, keep their colours under `NO_COLOR` (Jakob,
  2026-10-15). For the mascot this is superseded by ADR 41, and that
  by ADR 43: under `NO_COLOR` the mascot is an outline without any
  colour.
- crossterm 0.29 reads `NO_COLOR` too, and then writes every colour as
  an empty SGR sequence, which a terminal reads as a reset of all
  attributes. RGB colours were lost with it, Kitty images with their id,
  and a cell that changed colour lost the bold, dim or reversed written
  just before its colour. asqr forces crossterm's colour output on, so
  the drawn screen alone decides what shows.

A screen without an image has no RGB colour, and a test pins that. A
colour drawn in RGB outside an image would keep its colour under
`NO_COLOR`. A `ratatui-image` release that names Kitty images any other
way has to be checked against the pass.
