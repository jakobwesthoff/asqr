# 14. Lock a queue to one asqr instance

Date: 2026-09-27

## Status

Accepted

## Context

Two TUIs on the same queue would show the same sessions and race to
submit them.

## Decision

A lock file in the queue allows only one running asqr instance per
queue. A second instance names the process that holds the lock and
exits. A lock left behind by a process that no longer exists is taken
over.

Rejected: allowing several instances and letting the first submit win.

## Consequences

Answering on two machines needs two queues. Commands that only ask or
read (`ask`, `wait`, `result`, `paths`) do not take the lock.

## Amendment (2026-09-27, adversarial review F2)

The "stale lock" approach, a pid check with takeover, is replaced. Pids
get reused, two instances starting at once could both take over, and a
pid means nothing on a shared filesystem.

- The TUI takes an advisory lock on `<queue>/lock` with
  `std::fs::File::try_lock` (flock, stable since Rust 1.89).
- The kernel releases the lock when the process ends.
- `WouldBlock` means another instance runs. Any other error is a plain
  failure.
- The pid and host name in the file are only for the "held by" message.
- The lock is per host, so queues on network or synced filesystems are
  unsupported.
- `prune` and `ask --force` do not take the lock.
