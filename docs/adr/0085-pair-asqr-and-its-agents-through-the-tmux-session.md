# 85. Pair asqr and its agents through the tmux session

Date: 2026-10-30

## Status

Accepted

Amends [5. Keep all queues under one platform data root](0005-keep-all-queues-under-one-platform-data-root.md):
inside tmux the queue comes from the pane's tmux session

## Context

- A person who works in several tmux sessions runs an asqr in each, and
  the agents of a session should ask into that session's asqr, never
  into another's. With `--queue` and `ASQR_QUEUE` alone, both sides need
  the same name by hand, and an agent has no way to learn it.
- A process in a tmux pane has `TMUX` and `TMUX_PANE`; the commands an
  agent in Claude Code runs keep them. `tmux display-message -p -t
  "$TMUX_PANE" '#{session_name}'` names the pane's session, and `tmux
  show-environment -t "$TMUX_PANE"` reads the session's variables live,
  also for processes started before a variable was set.
- Without `TMUX_PANE`, tmux answers for the most recently active
  session, and for a pane that is gone it prints an empty name.
- tmux takes session names that are no valid queue names, such as
  `my work`.
- Plain terminal tabs have nothing that groups a tab of asqr with the
  tabs of its agents.

## Decision

- Order: `--queue`/`--dir`, then `ASQR_QUEUE`/`ASQR_DIR`, then the
  pane's tmux session variable `ASQR_SESSION_QUEUE`, then the tmux
  session's name, then `default`. tmux is asked only inside tmux, with
  `TMUX` and `TMUX_PANE` set, only when no flag and no variable chose,
  and only by commands that work on a queue. It works without any
  setup.
- A session name that is no valid queue name becomes one: each run of
  other characters turns into `-`, a leading `.` into `_`, and the name
  is cut at 200 bytes.
- `ASQR_SESSION_QUEUE` is read only from tmux, never from the process
  environment. A value that is no valid queue name is a usage error.
- When tmux cannot be asked from inside tmux, asqr behaves as outside
  tmux, without a word: outside tmux, and in other multiplexers,
  everything works through the default queue and the flags and
  variables anyway, and the pairing is convenience on top of them. tmux
  gets 500 ms to answer.
- In advice, a queue from the tmux session is reached by plain `asqr`;
  the default queue taken because tmux did not answer is named with
  `--queue default`.
- Outside tmux nothing changes. Other multiplexers pair nothing.

## Consequences

- An agent inside tmux passes no queue, and its subagents inherit the
  pane.
- Session names that turn into the same queue name, such as `my work`
  and `my-work`, share a queue; on a filesystem that ignores case, as
  macOS's by default, so do names that differ only in case, and a
  session named `Default` uses the default queue's directory.
- An agent outside tmux, while the person's asqr runs inside it, asks
  into `default`.
