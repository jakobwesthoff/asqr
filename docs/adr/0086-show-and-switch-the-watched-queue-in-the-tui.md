# 86. Show and switch the watched queue in the TUI

Date: 2026-10-30

## Status

Accepted

Amends [38. Give notices kinds and confirm a finished session](0038-give-notices-kinds-and-confirm-a-finished-session.md):
a notice that names a queue keeps the name and cuts the text after it
first

## Context

- One asqr watches one queue (ADR 14), and inside tmux each tmux
  session pairs with its own queue (ADR 85). Sessions can wait in a
  queue no asqr watches: one asked from outside tmux, one left in a
  queue the person switched away from, one in a queue whose asqr quit.
- The status line shows notices and messages until the next key; most
  of the time it is empty.
- Without a way to move, the person quits asqr and starts it on another
  queue, after first finding out which queues hold anything.
- Outside tmux nothing tells the person which queues exist when asqr
  starts.
- asqr's own environment says nothing about the shell an agent runs in:
  `ASQR_QUEUE` set where asqr starts may be unset for the agent, and
  the other way round.

## Decision

The other-queues line:

- While the status line has no notice and no message, it says how many
  sessions wait in named queues no asqr watches, this asqr's queue left
  out: `2 sessions wait in asqr-todos   w  queues`, or `5 sessions wait
  in 3 other queues   w  queues`. The number is bold, the words dim and
  `w` in the key style. Nothing shows at zero. Where `w` does not open
  the switcher, in a field, in the focused pane, in the full-screen
  view, under an overlay and over the empty queue, whose key bar names
  `w`, the line shows without `w  queues`.
- It is news, never a warning: several queues can be fine, so it takes
  no colour beyond bold and dim, and it never rings or notifies.
- A session counts until it is answered or withdrawn, however old.
  Outside tmux, the sessions in `default` of agents never told a queue
  count while this asqr watches another queue.
- asqr looks at the queues when it starts, when the switcher closes and
  every 5 seconds; a queue that cannot be read counts as empty.
- Where the line does not fit, a single queue's name is cut from its
  start down to 12 columns, then the queues read `other queues`, then
  the line reads `N wait elsewhere`; `w  queues` stays.

The switcher:

- `w` opens it where `L` opens the session list, and over the empty
  queue. One frame in the middle of the screen lists the queues this
  asqr can switch to, sorted by name with `default` among them, `N
  waiting` above 0, `◀ this asqr` bold in the primary colour at the end
  of the watched queue's row, and `✎ New queue: —` last. A queue given
  by `--dir` at the start stays its first row for the run, by its path.
  Queues another asqr watches stand apart below a blank row, dim, with
  `watched in tmux <session>` or `watched by another asqr`, and the
  cursor skips them, so every row the cursor reaches does what `enter`
  says.
- Where a queue came from shows only where it changes what a switch
  does, in the footer on the frame's bottom edge, which says what
  `enter` does on the row under the cursor. Inside tmux: `agents in
  tmux session <s> follow this asqr`, and on a `--dir` row `agents in
  tmux session <s> follow only a named queue`. Outside tmux: `tell your
  agents: --queue <name>`, none on `default` and none on the new-queue
  row while no name is typed, and on a `--dir` row `tell your agents:
  --dir <path>`. A path or a name too long for the footer is never
  shortened there, as an agent cannot use a shortened one: the footer
  reads `tell your agents --dir with this path` or `tell your agents
  --queue with this name`, and the row shows as much of it as fits.
- The frame keeps one width while it is open, as wide as its rows and
  every row's footer, so nothing jumps as the cursor moves; a long new
  name can widen it up to the screen.
- The switcher shows no line about `ASQR_QUEUE`; `asqr ask` names it
  instead, where the agent's own environment is (ADR 85).
- The new-queue row is a field as soon as the cursor stands on it.
  `enter` creates the queue and switches to it; a name of an existing
  queue's directory switches to that queue. A character a queue name
  cannot hold, and the 201st, are refused with a notice.
- A switch takes the new queue's lock, makes its directories, watches
  its inbox and reads it, then saves the drafts of the old queue's
  sessions that hold answers, before it lets go of the old queue, and
  pins a named queue last inside tmux. A step that fails leaves asqr on
  the old queue with a notice. After a switch the status line reads
  `watching <q> · N sessions wait in <old>`.
- Where a notice that names a queue does not fit the status line, the
  text after the name is cut first, at its end with `…`, and the name is
  cut only where it does not fit alone, without that text; a notice that
  names a session keeps the rule of ADR 38.

The start picker:

- Outside tmux, with no queue chosen and a queue besides `default` that
  no other asqr watches, `asqr` shows the switcher's list before it
  takes a lock, with `enter  watch` and `q  quit`. The cursor starts on
  `default` where no other asqr watches it, and the footer follows the
  cursor as in the switcher outside tmux: `tell your agents: --queue
  <name>`, none on `default`. A queue that cannot be taken leaves the
  picker open with the switch's notice.

## Consequences

- A queue with sessions nobody sees shows up on every asqr's screen
  until someone answers or withdraws them.
- The person moves between queues without restarting asqr.
- Inside tmux a switch to a named queue moves the agents of the session
  with it.
