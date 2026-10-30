# 81. Name the chosen queue

Date: 2026-10-30

## Status

Accepted

Amends [40. Set the terminal title](0040-set-the-terminal-title.md):
the title names a queue other than the default one

## Context

- asqr watches one queue: the queue named `default`, a queue named with
  `--queue` or `ASQR_QUEUE`, or a directory given with `--dir` or
  `ASQR_DIR` (spec section 5.2). A queue has one asqr at a time
  (ADR 14), so a person who works with several queues runs an asqr for
  each, in windows or tabs of their own.
- The title bar names the queue, dim:
  `asqr · queue: <queue> · N sessions waiting`. The terminal title,
  which a tab shows, reads `asqr · N waiting` for every queue (ADR 40),
  so the tabs of asqr on different queues look alike.
- A queue name follows the id syntax, at most 200 bytes. A `--dir` path
  has no limit of its own.
- crossterm's `SetTitle` writes the title into OSC 0 as it is given,
  and a BEL or an ESC ends that sequence. Ghostty drops a title of 256
  bytes or more (ADR 40).

## Decision

The chosen queue:

- Every queue but the one named `default` is chosen, also when
  `--queue default` names that one. The name is compared exactly: on a
  case-sensitive filesystem `--queue DEFAULT` is a queue of its own in
  a directory of its own. A queue given by `--dir` or `ASQR_DIR` is
  always chosen, also a directory that holds the default queue.
- A chosen queue goes by its name, or for `--dir` by the path as given,
  the same text the title bar shows.

The terminal title:

- The default queue's title stays `asqr · N waiting`. A chosen queue's
  title is `asqr · <queue> · N waiting`.
- Control characters in `<queue>` show as in text entries (spec section
  8.4): a C0 control and delete as their pictures from the Control
  Pictures block, a C1 control as `�`.
- `<queue>` takes at most 40 columns. A longer one shows its end behind
  `…`, cut at a `/` where one is in reach, as the title bar cuts it.
  Terminals and tmux cut a long tab title at their own width, and 40
  columns leave room for `asqr · ` and ` · N waiting` in a tab of usual
  width.
- A `/` at the end of a path is no place to cut, in the title bar too:
  the name before it would go.
- Where the whole title would still reach 256 bytes, as with a name of
  many combining marks, which take no column, `<queue>` gives up more
  characters from its start, so Ghostty shows the title.

## Consequences

- A queue name or path longer than 40 columns shows only its end in the
  tab title.
- A relative `--dir` shows as given, so asqr on `--dir .asqr` in two
  projects shows the same title in both.
- On a case-insensitive filesystem, `--queue DEFAULT` watches the
  default queue's directory and is named all the same.
