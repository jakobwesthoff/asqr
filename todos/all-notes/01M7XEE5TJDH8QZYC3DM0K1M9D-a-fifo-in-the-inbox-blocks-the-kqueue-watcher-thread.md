---
title: "A FIFO in the inbox blocks the kqueue watcher thread"
kind: bug
component: tui
impact: low
origin: implementation
---
# A FIFO in the inbox blocks the kqueue watcher thread

Found on 2026-11-20 while moving the inbox watcher to kqueue (ADR 113,
commit 39bee22, released in 0.19.0). notify's kqueue backend opens every
new entry of a watched directory with `File::open` to add a watch for it.
On a FIFO that call blocks until a writer appears, so a FIFO dropped into
the inbox stalls notify's thread and the TUI stops seeing new sessions.

## Problem

`src/tui/watch.rs` adds its own per-file watches only for regular files,
but the directory watch itself goes through notify, whose kqueue backend
opens the new entry before asqr sees it. Read in notify's source, not
reproduced in asqr (2026-11-20).

## Impact

Nothing in asqr creates FIFOs in the inbox; it takes a stray `mkfifo` or
another tool writing there. The TUI then shows no new sessions until a
writer opens the FIFO or asqr restarts.

## Suggested fix

Check whether notify can be told not to open new entries (or opens them
non-blocking in a newer release); otherwise watch the inbox directory
with a small kqueue watch of asqr's own and keep notify only for the
per-file watches. Test: create a FIFO in the inbox, then drop a session,
and require the session to appear.
