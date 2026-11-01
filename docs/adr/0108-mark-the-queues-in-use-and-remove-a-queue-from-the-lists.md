# 108. Mark the queues in use and remove a queue from the lists

Date: 2026-11-01

## Status

Accepted

Amends [85. Pair asqr and its agents through the tmux session](0085-pair-asqr-and-its-agents-through-the-tmux-session.md):
the removal of named queues that hold nothing, a dry run unless `--yes`
since ADR 97, counts the `active` marker as no data

Amends [86. Show and switch the watched queue in the TUI](0086-show-and-switch-the-watched-queue-in-the-tui.md):
the switcher and the start picker list the named queues in use, and
`backspace` there removes a queue after a question

## Context

- asqr makes a named queue on its first use: `--queue <name>`, a tmux
  session's name (ADR 85) and the switcher's new-queue row (ADR 86).
  `asqr queues`, the switcher and the start picker list every named queue
  under the root, so queues that were used once stay in those lists.
- `asqr prune` removes the named queues that hold no file but their lock
  files (ADR 85, 97). A queue that was used holds its finished sessions,
  which `asqr history` and `asqr replay` show (ADR 101).
- `QueueLocation::create_layout` makes a queue's directories. Its callers
  are `ask`, `ask --replaces`, the start of an asqr on a queue and a switch
  to one, each under the drafts lock or the queue lock.

## Decision

- A queue and its archive are apart: removing a queue takes it out of the
  lists for new work and keeps everything it holds, so its finished
  sessions stay for `asqr history` and `asqr replay`, until its next use
  brings it back.
- A file `active` in a named queue's directory marks the queue as in use.
  `create_layout` writes it, so every use for new work writes it, under
  the lock that use holds. A queue given by `--dir` gets none.
- A named queue is in use while it carries `active`, while a session waits
  in it or an asqr watches it; the default queue always is. `asqr queues`,
  the switcher and the start picker list the queues in use, and `asqr
  queues --removed` the other named queues. The switcher's `--dir` row
  stays for the whole run.
  `history --all`, the search of `replay`, the search of `wait`, `result`
  and `withdraw` in other queues, `compact --all` and `prune` take every
  queue.
- `asqr queues remove <name>` deletes `active` and prints what it did, at
  once and without a dry run, as it names exactly one queue. It holds the
  queue lock and the drafts lock from its look at the inbox to the
  deletion, so an `ask` or a start meanwhile is either seen or writes
  `active` again.
- `backspace` on a queue's row in the switcher and the start picker asks
  in the status line, with its keys in the key bar; `enter` or `y`
  removes, any other key keeps. The switcher and the picker refuse a
  queue in which sessions wait and name the command with `--force`, so
  rejecting them stays on the command line.
- A queue in which a session waits is refused; `--force` first rejects
  each waiting session on the path of a finish, with a reason that names
  the removed queue and the answers of its draft, so the asker's `wait`
  ends with 10 and reads why. A session that recovery would end is ended
  as recovery ends it.
- Never removed: a queue an asqr watches, the default queue, a directory
  that is a symbolic link, a queue given by `--dir`. A tmux session's pin
  of a removed queue stays.
- `prune` counts `active` as no data, as it counts the lock files.
- No migration runs. A queue made before `active` carries none until its
  next use, since an asqr has to be started on a queue, or a session
  asked into it, for it to matter. Until then it is listed while a
  session waits in it or an asqr watches it.
- asqr 0.16.0 and earlier in the same root do not write `active`, list
  every queue and never delete the marker. A queue only they use shows in
  the lists of a newer asqr while a session waits in it, while they watch
  it, or as the default queue. Their `prune` counts `active` as a file and
  keeps an empty queue that carries one.

Rejected: a marker for removed queues; moving a removed queue's archive
to a place of its own; finishing the sessions `--force` ends as
`withdrawn`, which says the asker took them back.
