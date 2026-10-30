---
title: "Let several asqr instances run side by side and route each agent to the right one"
kind: feature
component: cli
status: needs-discussion
origin: idea
tags: [ux]
---
# Let several asqr instances run side by side and route each agent to the right one

Raised by the user on 2026-10-29, nothing decided yet. Someone who works
in several terminal sessions at once wants one asqr per session, and
wants every agent to put its questions into the asqr of the session it
runs in. Today that takes manual queue names on both sides, and an agent
has no way to find out which queue it should use. Starting the right
asqr has to be easy too, maybe through a picker.

## The setup that motivates it

The user often has several tmux sessions open. Each has a few windows:
Claude Code runs in one, two or three of them, and asqr runs in another.
Whenever the user switches to that tmux session, the asqr window shows
what the agents of that session want answered.

A second tmux session works on something else, with its own agents and
its own asqr window. Questions from the agents in session A belong in
the asqr of session A, and questions from session B in the asqr of
session B. Never mixed, never all in one.

## What exists today

- Named queues: `--queue <name>` and `ASQR_QUEUE` pick a queue under the
  platform data root, `--dir` and `ASQR_DIR` a directory (ADR 0005,
  `src/queue/location.rs`, `QueueLocation::resolve`). Flags win over the
  environment, the environment over the `default` queue.
- One asqr per queue: the TUI holds a lock on `<queue>/lock`, and a
  second asqr on the same queue names the holder and exits (ADR 0014).
- `asqr ask` warns "no asqr is watching queue ..." when nobody holds the
  lock.
- The skill (`skills/asqr/SKILL.md`, "Queues") tells the agent to "use
  the queue the person's `asqr` watches", but gives it no way to learn
  which one that is.

So separate instances already work when both sides set the same
`ASQR_QUEUE` by hand. What is missing is the pairing: naming a queue,
getting that name to the agents of one session, and starting the
matching asqr without typing it.

## Ideas so far

From the user:

- The agents name the queue they use themselves, and that name is how
  asqr and agents find each other.
- asqr shows a selection when it starts, so the person picks which
  queue this instance watches instead of typing a name.

Further directions to weigh, none checked against the code or tried yet:

- Derive the queue name from the tmux session. Inside a tmux pane `$TMUX`
  is set, and `tmux display-message -p '#S'` prints the session name, so
  asqr and an agent in the same tmux session could both arrive at the
  same name without anyone choosing it.
- Set `ASQR_QUEUE` per tmux session with `tmux set-environment`. Only
  processes started afterwards inherit it, so agents already running
  keep their old value.
- Derive the name from the repository or working directory instead of
  the terminal session. That pairs by project, which is a different
  grouping than the one the user described.
- A command that lists the queues with the number of waiting sessions
  and whether an asqr watches each one. It would feed the start picker,
  and an agent could use it to find the watched queues. `ask` could name
  the watched queues in its "nobody is watching" warning.

## Open questions

- Who names a queue: the person starting asqr, the agent, or a rule
  that derives it from the environment (tmux session, repository)?
- How does an agent learn the name: an inherited environment variable,
  a query to asqr, a rule the skill teaches, or asking the person?
- What happens when an agent asks into a queue nobody watches while
  other queues are watched?
- What does the start picker show (known queues, waiting sessions,
  watched or free), and can it create a new queue?
- How tmux-specific may this be? Plain terminal tabs and other
  multiplexers need a way too, even if it is less automatic.
- Queues that nobody uses any more pile up under the data root. Does
  `prune` or the picker clean them up?
- The skill changes with whatever is decided, since agents follow it to
  pick a queue.

## Discussion planned (2026-10-30)

Jakob picked this as the larger design topic to discuss soon
(asqr session 01M6628SFCCK00ENY6YV0XQZR1). It relates to the queue name on the waiting screen, which
is being designed at the same time.

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

## Relations

- Relates to: [Decide whether to build a terminal dashboard that agents write their status to](01M5VR1S0NSFH97XRWCKNBG7DR-decide-whether-to-build-a-terminal-dashboard-that-agents-write-their.md), the same question of which agents feed which view
