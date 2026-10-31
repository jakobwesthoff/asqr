# 93. Show a `--dir` queue's path absolute, with `~` for home

Date: 2026-10-31

## Status

Accepted

Amends [81. Name the chosen queue](0081-name-the-chosen-queue.md):
a `--dir` queue goes by its absolute path with `~` for the home folder,
not by the path as given

Amends [86. Show and switch the watched queue in the TUI](0086-show-and-switch-the-watched-queue-in-the-tui.md):
the footer on a `--dir` row prints the path absolute and as a shell
reads it back

## Context

- A queue given with `--dir` or `ASQR_DIR` has no name, so asqr names it
  by its directory. The person saw the directory as it was typed.
- `.asqr` from an `.envrc` reads the same in every project, so two asqr
  on two projects' `.asqr` showed the same queue.
- `--dir .` gave "Nothing to answer in queue .." with a double stop.
- `asqr ask` already gives agents the absolute path
  (`src/cli/hint.rs`).
- The switcher footer printed the row's label after `--dir`. For a
  relative path that told an agent to run `--dir .asqr` from whatever
  directory it stands in.
- The footer's text is a command to paste into a shell. `~` expands only
  outside quotes, so a quoted `'~/my q'` stays literal.

## Decision

- Every place that names a `--dir` queue shows its path absolute, with
  `~` for the home folder: the waiting screen's sentence, the title bar,
  the terminal title, the switcher row and the status line's notice after
  a switch. This tells two projects' `.asqr` apart, matches what agents
  get from `asqr ask` and ends the double stop in "queue ..".
- Rejected: keep the path as typed, and show the last component only
  (two `.asqr` still look alike, and `.` has none).
- A path too long for a title bar or a tab title keeps the cut of ADR
  81: from the left, at a `/`, keeping as much of the end as fits.
  Rejected: always the last two components.
- The waiting screen names the path in the title bar and in the
  sentence. Rejected: the title bar only, which would reverse part of
  ADR 81 for `--dir` queues.
- Only the display helpers make the path absolute, with
  `std::path::absolute`: `queue_label` and the switcher's labels.
  `QueueLocation::resolve`, the queue's directory value, its lock and its
  inbox stay as they are. A trailing slash is dropped, and a path equal
  to the home folder shows as `~`.
- Symlinks are not resolved, so `/tmp/x` and `/private/tmp/x` stay
  different strings on macOS.
- The switcher footer uses the same absolute path, written as a shell
  reads it back: `~/` followed by the rest, quoted where the rest needs
  quotes (`~/'my q'`), for a path inside the home folder; the quoted
  absolute path otherwise; `<dir>` for a path that is not UTF-8 or holds
  a control character. `cli::hint` quotes a path the same way. A path
  too long for the footer keeps the fallback text of ADR 86.

## Consequences

- Two projects' `--dir .asqr` show two paths.
- A relative `--dir` goes through the current directory as the operating
  system reports it, while an absolute one keeps the text it was given.
  Where `HOME` names a symlinked spelling of the same folder, one queue
  shows `~` for the absolute spelling and the full path for the relative
  one.
- A `..` in a `--dir` path stays in the shown path.
- The header beside a session names the queue as the title bar does, so
  it shows the absolute path too.
- The switcher and the status line's other-queues line still cannot list
  a `--dir` queue that another asqr watches, as asqr has no record of
  other instances' `--dir` queues.
