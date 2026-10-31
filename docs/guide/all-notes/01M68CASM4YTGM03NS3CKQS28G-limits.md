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

## Large entries

asqr cuts or simplifies what is large:

- A file longer than 8 MiB is read for its first 8 MiB. A dim "first 8 MiB
  shown" heads the content and a dim "… the rest is cut after 8 MiB" ends
  it.
- A text of more than 500,000 lines, inline or in what is read of a file,
  shows its first 500,000 lines, headed by a dim "first 500,000 lines
  shown" and ended by "… the rest is cut after 500,000 lines". An image's
  caption is cut the same way.
- A Markdown entry or an image caption above 1 MiB is not rendered. It
  shows as plain text below a dim "over 1 MiB: shown as plain text".
- A code entry above 1 MiB, and a diff whose two sides together are above
  1 MiB, show without highlighting. Highlighting covers the first 50,000
  lines, and a dim "no highlighting after line 50,000" stands where it
  stops.
- Should the Markdown parser fail on an entry, asqr shows it as plain
  text below a dim "Markdown failed: shown as plain text". Should it fail
  on the intro, a question text, an option label or a description, that
  text shows as written, without Markdown.
- An image file larger than 64 MiB is not read, and a card stands in its
  place.

`asqr validate` and `asqr ask` warn about the entries above 1 MiB.

## What a session cannot hold

- Every question is optional. There is no way for an asker to force an
  answer, and a skipped question comes back as skipped.
- A question, and each of its options, carries at most one context entry.
- The kinds of questions are single choice, multiple choice, text and info.
- Sessions and results are JSON. No other format is read or written.

## What asqr does not have

- Networking, several people on one queue, and sockets.
- An MCP server. Agents ask through commands and files, as
  [Common questions](01M690YKZXCMGCA31H3HTHFYJE-common-questions.md)
  explains.
- `asqr answer`, answering one file in the current terminal without a
  queue.
- Partial submits.
