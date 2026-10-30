# 86. Show and switch the watched queue in the TUI

Date: 2026-10-30

## Status

Accepted

## Context

- One asqr watches one queue (ADR 14), and inside tmux each tmux
  session pairs with its own queue (ADR 85). Sessions can wait in a
  queue no asqr watches: one asked from outside tmux, one left in a
  queue whose asqr quit.
- The status line shows notices and messages until the next key; most
  of the time it is empty.

## Decision

- While the status line has no notice and no message, it says how many
  sessions wait in named queues no asqr watches, this asqr's queue left
  out: `2 sessions wait in asqr-todos`, or `5 sessions wait in 3 other
  queues`. The number is bold and the words dim. Nothing shows at zero.
- It is news, never a warning: several queues can be fine, so it takes
  no colour beyond bold and dim, and it never rings or notifies.
- A session counts until it is answered or withdrawn, however old.
  Outside tmux, the sessions in `default` of agents never told a queue
  count while this asqr watches another queue.
- asqr looks at the queues when it starts and every 5 seconds; a queue
  that cannot be read counts as empty.
- Where the line does not fit, a single queue's name is cut from its
  start down to 12 columns, then the queues read `other queues`, then
  the line reads `N wait elsewhere`.

## Consequences

- A queue with sessions nobody sees shows up on every asqr's screen
  until someone answers or withdraws them.
