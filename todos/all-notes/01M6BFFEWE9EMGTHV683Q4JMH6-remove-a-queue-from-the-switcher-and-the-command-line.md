---
title: "Remove a queue from the switcher and the command line"
kind: feature
component: queue
origin: request
tags: [ux]
---
# Remove a queue from the switcher and the command line

asqr creates a named queue on first use (`--queue <name>`, a tmux
session's queue, the switcher's `✎ New queue` row), but nothing removes
one. Queues pile up in the switcher, `asqr queues` and the start picker.
The owner asked on 2026-11-01 for a way to remove queues: an empty queue
from the switcher and from the command line, and with a force argument
also a queue that is not empty.

## What the owner said (2026-11-01)

- Removing should work in the queue switcher and on the command line.
- The command line gets a force argument that removes a queue that is
  not empty too.
- Maybe in the TUI as well; `backspace` on a queue's row in the switcher
  seemed a good key to the owner, open for discussion.

## Lead's view (2026-11-01), for the discussion

- `backspace` fits the switcher: on a queue row it types nothing, and it
  is the key people press to delete in a list on macOS. On the
  `✎ New queue` row it keeps deleting typed characters. It should ask
  before it removes (a confirmation line in the switcher's footer,
  `enter`/`y` to remove, anything else to keep), since a removed queue
  takes its archive, and with it the history and replays, along.
- The switcher removes only empty queues; for any other it says why not
  and names the command with `--force`. That keeps the destructive case
  on the command line, where it is deliberate.
- What "empty" means needs deciding: no waiting session and no draft
  surely; whether an archive of finished sessions counts (it holds the
  history `asqr history` and `replay` show).
- Never removable: the queue an asqr watches (its own included, so the
  switcher refuses the row marked `◀ this asqr`), a queue another asqr
  watches, and probably `default`. A queue pinned in a tmux session
  would come back on the next start there; decide whether removing it
  unpins it.
- Command line shape, for example `asqr queues remove <name>`
  (`--force` for a non-empty one), or a new `asqr remove-queue`; the
  `--dir` queues are a directory the user named and may be left out.
- Spec sections 5 (queues and files), 6.5 (`asqr queues`) and 7.6 (the
  switcher) and the start picker change with it.

## Decisions (2026-11-01, the owner, asqr session 01M6BG39EPAPHYHPJX7F8YBCMZ)

- Removing a queue does not take its archive along. What happened in a
  queue stays: its finished sessions remain for `asqr history` and
  `asqr replay`, also for queues no longer in use. Removing only takes
  the queue out of the lists for new work (switcher, start picker,
  `asqr queues`) until it is created again by its next use. Owner's
  words: archives of queues not used any more but once used a lot must
  stay; queue and archive are separate.
- In the switcher, `backspace` on a queue's row removes it; the start
  picker does the same, with the same rules.
- The confirmation shows in the status line, its keys in the key bar:
  `enter` or `y` removes, any other key keeps.
- Without `--force`, a queue can be removed when no session waits in it.
- On a queue with waiting sessions the switcher refuses and names the
  command with `--force`.
- The command is `asqr queues remove <name> [--force]`, next to `asqr
  queues`; it removes at once (no dry run), naming exactly one queue, and
  prints what it did.
- `default` can never be removed.
- A tmux session's pin stays: the queue comes back empty when something
  is asked there.
