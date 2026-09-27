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
