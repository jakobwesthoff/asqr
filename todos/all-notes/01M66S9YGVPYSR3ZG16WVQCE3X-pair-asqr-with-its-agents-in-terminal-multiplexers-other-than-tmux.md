---
title: "Pair asqr with its agents in terminal multiplexers other than tmux"
kind: feature
component: cli
status: needs-discussion
horizon: someday
origin: discussion
tags: [ux]
---
# Pair asqr with its agents in terminal multiplexers other than tmux

asqr pairs with the agents of the same tmux session without any setup
(ADR 0085, `src/pairing.rs`): a command with no queue chosen uses the
queue the tmux session's pin `ASQR_SESSION_QUEUE` names, else a queue
named after the session, and the TUI pins the queue it watches. Other
multiplexers get no such pairing; outside tmux the person tells the
agents `--queue <name>`, helped by the start picker and the switcher's
footer. While several asqr instances were designed (2026-10-29/30),
Jakob decided that automatic pairing only in tmux is fine for now, with
room left for other multiplexers later ("later, keep open").

## Context

- The seam is the `Tmux` trait in `src/pairing.rs`: the session name, the
  pin read and written through the session's environment, the pane of the
  asqr that holds a lock. A second multiplexer would need the same three
  things.
- Outside tmux everything keeps working; pairing is convenience on top
  (ADR 0085).

## Open questions

- Which multiplexers matter (zellij was named in the discussion), and
  does each expose a session name and an environment per session that
  every pane of it reads, as tmux does with `set-environment`?
- Detection: which variables tell asqr it runs inside which multiplexer.

## Re-checked (Jakob, 2026-10-31, in the chat)

Kept for later, tmux-only pairing stays (ADR 85). The lead's check that
day: zellij documents no per-session variable asqr could write, so a pin
as tmux has would have to live in a file under the data root; pairing
through `ZELLIJ_SESSION_NAME` alone would need no pin. Rejected for now:
zellij by session name, zellij with a pin, zellij and GNU screen.

## Revisit when

No trigger known; reconsider at the next sweep.
