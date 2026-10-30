---
title: "Decide how the waiting screen and the tab title show a `--dir` queue's path"
kind: improvement
component: tui
status: needs-discussion
horizon: next
origin: request
tags: [ux]
---
# Decide how the waiting screen and the tab title show a `--dir` queue's path

Requested by Jakob on 2026-10-30. A queue given with `--dir` (or
`ASQR_DIR`) is named on the waiting screen and in the terminal tab
title by its path, as given. A path can be long, and some paths read
alike in every project. How such a path should show is not decided yet.

## What landed

The rest of the original request ("Show the watched queue name
prominently on the waiting screen") is done (ADR 81):

- 0b55cc6: the tab title names any queue but the default one,
  `asqr · <queue> · N waiting`, the queue cut from the left to 40
  columns, control characters as pictures, the whole title under 256
  bytes.
- The commit after it: the waiting screen splits its sentence for any
  queue but the default one, "Nothing to answer in queue **<queue>**."
  and "New sessions show up here as soon as they arrive.", the name bold
  in the primary colour, on a line of its own where it does not fit
  beside, cut from the left only where that line does not hold it.

## Decisions the open point relies on (2026-10-30)

- The default queue keeps today's screen and the title
  `asqr · N waiting`; an explicit `--queue default` counts as the
  default. The comparison is exact, so `--queue DEFAULT` is named.
- A `--dir` queue counts as chosen and shows its path for now, on the
  screen and in the tab title.
- On a screen too short for both, Talqi gives way and the name stays.

## Open question

How a `--dir` path shows. Known cases:

- A long path: on the screen it is cut from the left at the sentence's
  width (56 columns on a screen of 60), in the tab title at 40 columns,
  each at a `/` where possible, so only the path's end shows.
- A relative `--dir`, such as `.asqr` from an `.envrc`, shows as given,
  so two projects that use it look the same.
- `--dir .` gives "Nothing to answer in queue .." with a double stop.

Options raised so far: keep the path as given, show its last component,
show `~` for the home directory, or make a relative path absolute
(`std::path::absolute`, no filesystem access) before cutting it.

The queue switcher (`w`, ADR 86) shows a `--dir` queue by its path with
`~` for the home folder, cut from its start where its row is too
narrow, and outside tmux its footer names the whole path or points to
the row.
