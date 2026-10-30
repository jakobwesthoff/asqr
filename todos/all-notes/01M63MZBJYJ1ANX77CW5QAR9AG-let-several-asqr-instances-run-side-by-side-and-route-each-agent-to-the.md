---
title: "Let several asqr instances run side by side and route each agent to the right one"
kind: feature
component: cli
origin: idea
tags: [ux]
---
# Let several asqr instances run side by side and route each agent to the right one

Someone who works in several tmux sessions runs one asqr in each, and
every agent puts its questions into the asqr of the session it runs in.
The pairing through the tmux session is built (ADR 85); the queue
switcher, the start picker and the rest of the work below are still
open. The design is settled in the decisions at the end.

## What is done

- Inside tmux the queue comes from the pane's tmux session: the pin
  `ASQR_SESSION_QUEUE`, else the session's name made a valid queue name;
  the TUI pins its queue once it runs; a tmux that does not answer
  leaves asqr as outside tmux, without a word (spec 5.2, 7.1, ADR 85).
- The lock file names the holder's tmux pane; the refusal of a second
  asqr names its tmux session (spec 5.6).
- `asqr queues [--json]` (spec 6.5).
- `ask` names the watched queues when nobody watches, warns when the
  queue of its tmux session is watched from another tmux session, and
  gives the `ASQR_QUEUE` hint (spec 5.5, 6.7).
- The skill: pass no `--queue` unless told, never switch on your own.
- The screen behind every overlay is dim (ADR 84).

## What remains

- `wait` and `result` find a session in another named queue; `withdraw`
  and `ask --replaces` name that queue and refuse (the lead's T2 and its
  decision on withdraw).
- `asqr prune` removes unwatched queues that hold no file, with both
  locks following their path.
- The status line counts the sessions waiting in queues nobody watches.
- The queue switcher on `w`.
- The start picker outside tmux.

The implementation plan, with the agreed review fixes and the texts the
lead and the user settled, is
`~/playground/2026_10_30_asqr-todos-worked-off-by-agents/plans/instances-impl-plan.md`.

## Decisions (2026-10-30, asqr session 01M666SB2DM25EBG50X16CGMP3)

Design worked out in the conductor logbook of 2026-10-30
(`~/playground/2026_10_30_asqr-todos-worked-off-by-agents/plans/instances/design.md`).
Checked there: agents in Claude Code keep `$TMUX` and `$TMUX_PANE`;
`tmux display-message -p -t "$TMUX_PANE" '#S'` names the pane's session
in about 5 ms; `tmux show-environment -t "$TMUX_PANE" <var>` reads a
session variable live, also from processes started before it was set.

Jakob chose:

- Pairing model D: inside tmux asqr derives the queue from the pane's
  tmux session, and the TUI pins the queue it watches to that session
  (a tmux session variable); agents' commands read the pin. Order:
  `--queue`/`--dir`, then `ASQR_QUEUE`/`ASQR_DIR`, then the pin, then
  the session name, then `default`.
- It works without setup whenever asqr runs inside tmux.
- The pin stays when asqr quits.
- An `ask` into a queue nobody watches is queued with a warning that
  names the watched queues. The warning's last line must tell the agent
  to ask the person to start asqr in that session, not tell the agent to
  start it.
- A listing `asqr queues` (also `--json`).
- `asqr prune` removes unwatched queues that hold no file at all.
- A start picker outside tmux, when several queues exist.
- The skill: pass no `--queue` unless told; never switch to another
  session's watched queue on your own.

Jakob's notes:

- Outside tmux everything must keep working and stay usable, even where
  nothing is detected automatically. Automatic pairing only in tmux is
  fine for now; leave room for other multiplexers later.
- When tmux cannot be asked from inside tmux, asqr should behave
  sensibly, more or less as today: use the default queue (recorded as
  his direction; to confirm).
- Open: how exactly an agent in the same tmux session learns a queue
  chosen by hand, and how to choose the default queue on purpose (the
  pin question was left open for this). New idea: a queue selector the
  person opens with a shortcut inside the TUI, as a pop-up, to switch
  between queues, showing which session tmux detected.
- Other multiplexers: later, keep open.

## Decisions (2026-10-30, asqr session 01M6678BXTKR8GK72MB2828M8F)

- How a chosen queue reaches the agents, as explained to Jakob: the TUI
  stores its queue in a variable of its tmux session; every asqr command
  an agent runs in a pane of that session reads it from tmux. `asqr
  --queue default` pins the default queue on purpose.
- The TUI stores its queue in the session on every start inside tmux,
  also a queue derived from the session name.
- When tmux cannot be asked from inside tmux, asqr uses the default
  queue and warns, naming `--queue`.
- A queue switcher inside the TUI, opened with a key as a pop-up, comes
  with this work. Its design needs thought first: immediately
  understandable, not too complex, quick to use, and still showing which
  information came from where and what runs where, without overloading
  it. Jakob found the first mockup unclear. A multi-agent design
  discussion comes first, then options for him to choose and iterate on.
- The no-watcher warning's wording depends on whether tmux was detected.
  Outside tmux it tells the agent to ask the person to start `asqr` on
  that queue (`asqr --queue <name>`); inside tmux, to start `asqr` in a
  window of that session. Starting `asqr --queue <name>` is always an
  option and the warning may name it in both cases.

## Relations

- Relates to: [Decide whether to build a terminal dashboard that agents write their status to](01M5VR1S0NSFH97XRWCKNBG7DR-decide-whether-to-build-a-terminal-dashboard-that-agents-write-their.md), the same question of which agents feed which view
