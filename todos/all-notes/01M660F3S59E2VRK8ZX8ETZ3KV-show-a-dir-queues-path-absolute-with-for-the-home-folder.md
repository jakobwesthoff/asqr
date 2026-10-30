---
title: "Show a `--dir` queue's path absolute, with `~` for the home folder"
kind: improvement
component: tui
horizon: next
origin: request
tags: [ux]
---
# Show a `--dir` queue's path absolute, with `~` for the home folder

A queue given with `--dir` (or `ASQR_DIR`) has no name, so asqr names it
by its directory. The person sees that directory exactly as it was
typed, so `.asqr` from an `.envrc` reads the same in every project and
`--dir .` gives "Nothing to answer in queue .." with a double stop. The
agents meanwhile get the absolute path from `asqr ask`
(`src/cli/hint.rs:104-113`). Decided on 2026-10-30: every place the
person sees the path shows it absolute, with `~` for the home folder.

## Where the path shows today (main 1dcae03)

- Waiting screen sentence, "Nothing to answer in queue <path>.",
  `naming_lines` in `src/tui/render/empty.rs:75-93`.
- Waiting screen title bar, " asqr · queue: <path> · N sessions
  waiting", `draw_title` in `src/tui/render/screen.rs:555-572`, cut
  from the left to the room beside the count.
- Terminal tab title, `terminal_title` in `src/tui/run.rs:287-305`, cut
  to 40 columns (`TITLE_QUEUE_COLUMNS`) at a `/` where possible.
- Queue switcher row on `w` (ADR 86): the path with `~` for home
  (`src/tui/switch.rs:47-51`, `shown_path` in
  `src/tui/render/problem.rs:103-114`), cut from its start.
- Switcher footer outside tmux, "tell your agents: --dir <label>"
  (`src/tui/render/switcher.rs:145-147`). It prints the row's label, so
  a relative path tells an agent to run `--dir .asqr` from whatever
  directory it stands in. That is wrong today, whatever the form.
- The status-line notice after a switch names the old queue with
  `queue_label` (`src/tui/switch.rs:162-169`), which returns the
  directory as given.

The name for all of these comes from `queue_label` and the switcher
label, both of which use the directory as given.

## Decisions (2026-10-30)

From earlier work on this request (ADR 81), still binding:

- The default queue keeps today's screen and the title
  `asqr · N waiting`; an explicit `--queue default` counts as the
  default. The comparison is exact, so `--queue DEFAULT` is named.
- A `--dir` queue counts as chosen and is named on the screen and in the
  tab title.
- On a screen too short for both, Talqi gives way and the name stays.

Jakob, 2026-10-30, in the chat:

- **Form:** the waiting screen sentence, the title bar, the tab title,
  the switcher row and the switcher footer show a `--dir` path absolute,
  with `~` for the home folder. This tells two projects' `.asqr` apart,
  matches what agents get from `asqr ask`, and ends the "queue .."
  double stop. Rejected: keep the path as typed; show the last component
  only (two `.asqr` still look alike, and `.` has none).
- **Long paths:** keep today's cut. The title bar and the tab title each
  cut from the left at a `/`, keeping as much of the path's end as fits.
  Rejected: always the last two components.
- **Named twice:** the waiting screen keeps the path both in the title
  bar and in the sentence. Rejected: title bar only, which would reverse
  part of ADR 81 for `--dir` queues.

Lead, 2026-10-30:

- The path is made absolute with `std::path::absolute` in the display
  helpers only (`queue_label`, the switcher label), not in
  `QueueLocation::resolve`, so the queue's directory value, its lock and
  its inbox stay as they are. `std::path::absolute` does not resolve
  symlinks, so `/tmp/x` and `/private/tmp/x` stay different strings on
  macOS; that is accepted.
- The switcher footer uses the same absolute form, which fixes the
  relative path it tells agents today.
- A new ADR amends ADR 81 ("as given" and its consequence on relative
  paths) and ADR 86 (the footer); spec sections 7.5, 7.6 and 7.9 follow.

## Out of scope

asqr has no record of other instances' `--dir` queues, so the switcher
and the status line's other-queues line cannot list a `--dir` queue that
another asqr watches. Not part of this todo.

## Tests to update

The `--dir` title tests in `src/tui/run.rs`, the header test
`a_long_queue_path_gives_way_to_the_count` in
`src/tui/render/screen.rs`, the empty-queue snapshots under
`src/tui/render/snapshots/`, and the `--dir` switcher tests in
`src/tui/render/switcher.rs`.
