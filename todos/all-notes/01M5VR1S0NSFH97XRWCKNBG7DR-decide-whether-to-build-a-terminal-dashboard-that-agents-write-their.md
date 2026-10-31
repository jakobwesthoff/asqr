---
title: "Decide whether to build a terminal dashboard that agents write their status to"
kind: investigation
component: project
status: needs-discussion
horizon: someday
origin: idea
---
# Decide whether to build a terminal dashboard that agents write their status to

An idea from 2026-10-26, nothing decided yet. asqr lets an agent hand a
person a catalogue of questions. The counterpart would be a terminal
dashboard that agents write into while they run orchestrated work:
status messages, current lists, green and red indicators, whatever turns
out to be useful. The person watching keeps track of what the agents are
doing right now without reading each transcript.

## Context

When several agents work in parallel, such as conductor runs with one
agent per worktree, the only overview today is each agent's own output.
A shared board in the terminal would collect that state in one place.

It might also combine the input of several agents, or of several
separate sessions, into one view. How that works is open.

## Options

- A separate project next to asqr. Mark suggested a name that starts
  with a B, derived from "dashboard", to pair with the A of asqr.
  Spellings floated so far: boarder, boardr, boadr. None is settled.
- A feature inside asqr.

## Open questions

- What agents can write: free text, key-value status, lists, traffic
  lights, progress. And how the board lays these out.
- How several writers combine: one section per agent, per session, or
  something else.
- How agents deliver updates. asqr's on-disk session queue is one model
  to look at.
- When entries expire and how a finished run gets cleared away.

## Re-checked (Jakob, 2026-10-31, in the chat)

Left as it stands, the open questions unasked: Jakob expects the
dashboard to end up as an application of its own rather than part of
asqr.

## Revisit when

No trigger known; reconsider at the next sweep.
