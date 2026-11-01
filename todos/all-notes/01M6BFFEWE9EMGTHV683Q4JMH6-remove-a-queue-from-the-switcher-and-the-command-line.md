---
title: "Remove a queue from the switcher and the command line"
kind: feature
component: queue
status: needs-discussion
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

## Open questions for the owner

- The key in the switcher (`backspace`, or another), and whether it
  confirms.
- What counts as empty, especially the archive.
- The command's name and place, and what `--force` may remove.
- Whether `default` and pinned tmux queues can be removed.
