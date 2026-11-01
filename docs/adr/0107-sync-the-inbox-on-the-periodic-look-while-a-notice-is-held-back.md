# 107. Sync the inbox on the periodic look while a notice is held back

Date: 2026-11-01

## Status

Accepted

Amends [105. Carry the draft over to a session that replaces theirs](0105-carry-the-draft-over-to-a-session-that-replaces-theirs.md):
a notice held back by the drafts lock no longer waits only for the next
change of the inbox

Builds on [86. Show and switch the watched queue in the TUI](0086-show-and-switch-the-watched-queue-in-the-tui.md):
the look every 5 seconds also syncs the inbox in one case

## Context

- A sync that sees a session gone while the drafts lock is held and no
  session has arrived keeps the withdrawal notice for the next sync, as
  a replacement holds the lock while it withdraws the old session and
  writes the new file (ADR 105).
- The run loop syncs when the inbox watcher reports a change. A
  withdrawal that writes no new session file causes no further change.
- The lock is held by more than a replacement: the asker's second hold
  that records the files of a withdrawn session, the keep worker of asqr,
  a compaction, and a replacement that stopped before it wrote its file.
  The watcher reports the withdrawal 150 ms after the last change, so one
  of these can still hold the lock then.
- A sync under a lock that is still held waits for it, up to about 2
  seconds for each waiting session (spec section 5.6).
- A lock that nobody holds can look held for a moment: a process started
  on another thread keeps the open file description of a lock that was
  just let go of until it runs its program.

## Decision

- **The look syncs the inbox.** On each look at the other queues (every
  5 seconds), the TUI syncs the inbox when a notice is held back and one
  try finds the drafts lock free. The screen is drawn with what the sync
  changed.
- **A held lock costs one try.** While the lock is held, the look tries
  it once and syncs nothing.
- **The rule of ADR 105 stays otherwise.** A scan that sees the old
  session gone while the drafts lock is held, with no new session
  arrived, leaves the notice for the next scan; the next scan is the
  next change of the inbox or the next look.

## Consequences

- A held-back notice shows within 5 seconds of the lock being free,
  also when no file arrives.
- A sync that fails shows its error notice on each look while a notice
  stays held back.
- A look that syncs is not delayed by the watcher's 150 ms wait, so a
  file written in several steps by hand can be read before it is whole,
  as at the start of the TUI.
