---
title: "Limits"
tags: [guide/reference]
site:
  order: 5
---
What asqr does not do, and where it is tested.

## Where it runs

- macOS and Linux. Windows may work but is not tested.
- The terminal needs at least 60 columns by 15 rows.
- One asqr watches a queue, on one machine. Queues on network or synced
  filesystems, such as NFS, Dropbox and iCloud Drive, are not supported.

Inside tmux, images and desktop notifications need
`set -g allow-passthrough all`. The page on
[notifications, the terminal and tmux](01M68C1XVP8WV9Z6NWRJ1BEWT8-notifications-the-terminal-and-tmux.md)
has the details.

## What a session cannot hold

- Every question is optional. There is no way for an asker to force an
  answer, and a skipped question comes back as skipped.
- A question, and each of its options, carries at most one context entry.
- The kinds of questions are single choice, multiple choice, text and info.
- Sessions and results are JSON. No other format is read or written.

## What asqr does not have

- Networking, several people on one queue, and sockets.
- An MCP server.
- `asqr answer`, answering one file in the current terminal without a
  queue.
- Partial submits.
