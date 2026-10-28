# 63. Keep draft saves and replacements apart with a drafts lock

Date: 2026-10-28

## Status

Accepted

Amends [4. Hand sessions over through watched queue directories](0004-hand-sessions-over-through-watched-queue-directories.md)

## Context

The TUI saves a draft on every change. `asqr ask` replaces a waiting
session only while its draft holds no answer (ADR 4): it reads the
draft, then writes the new file over the waiting one. The TUI and `ask`
run in separate processes, so a draft the TUI saves between `ask`'s
look at the draft and its write is not seen, and the session is
replaced although its draft now holds an answer.

The TUI's finish of a session, its recovery of interrupted finishes and
its error result for an invalid file each look at the queue's files
and then write as well, while a command can change the same session
from another process.

## Decision

- An exclusive advisory lock on `<queue>/drafts.lock`, apart from the
  instance lock of ADR 14, keeps these apart. The TUI and the CLI are
  separate processes, and a check-then-write between them races.
- Each of these holds the lock from its first look at the files to its
  last write, a few file operations:
  - the TUI's draft save, which writes the draft only while the
    session's file is still in the inbox;
  - the TUI's finish, from reading the session file again to deleting
    the draft;
  - recovery, for each waiting session on its own;
  - the error result of an invalid file, written only while the file
    still holds the bytes that failed validation;
  - `asqr ask`, from its look at the draft to its write into the inbox.
- The lock is tried for about 2 seconds (100 tries 20 ms apart). When
  it stays taken, the operation writes nothing and says "the queue is
  busy": `ask` refuses with exit 1, the TUI keeps the answers and shows
  an error notice, and recovery leaves the session for its next run,
  which the next change of the inbox starts.
- The file stays after use, like `<queue>/lock`. `asqr paths` does not
  list it.

## Consequences

- A change to a session whose file has left the inbox saves no draft.
- A TUI of an asqr from before this decision does not take the lock.
- Until a skipped recovery runs, the TUI shows that session like a
  waiting one.
