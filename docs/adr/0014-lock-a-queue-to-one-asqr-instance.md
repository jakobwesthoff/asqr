# 14. Lock a queue to one asqr instance

Date: 2026-09-27

## Status

Accepted

Amended by [85. Pair asqr and its agents through the tmux session](0085-pair-asqr-and-its-agents-through-the-tmux-session.md):
the lock file names the tmux pane of its holder, and a lock follows its
path

## Context

Two TUIs on the same queue would show the same sessions and race to
submit them. A lock file with a pid check and takeover of a stale lock
does not hold: pids get reused, two instances starting at once could
both take over, and a pid means nothing on a shared filesystem.

## Decision

One running asqr instance per queue. The TUI takes an advisory lock on
`<queue>/lock` with `std::fs::File::try_lock` (flock, stable since Rust
1.89).

- The kernel releases the lock when the process ends.
- A `WouldBlock` that lasts through 200 ms of tries means another
  instance runs: the second instance names the process that holds the
  lock and exits. The tries cover the moment `ask` or `status` holds
  the lock shared to see who watches. Any other error is a plain
  failure.
- The pid and host name in the file are only for that "held by" message.
- The lock is per host, so queues on network or synced filesystems are
  unsupported.
- Commands that only ask or read (`ask`, `wait`, `result`, `paths`) do
  not take the lock, and neither do `prune` and `ask --force`.

Rejected: allowing several instances and letting the first submit win.

## Consequences

Answering on two machines needs two queues.
