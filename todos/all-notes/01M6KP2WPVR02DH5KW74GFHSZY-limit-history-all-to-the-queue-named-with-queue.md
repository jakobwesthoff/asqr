---
title: "Limit history --all to the queue named with --queue"
kind: improvement
component: cli
origin: request
tags: [ux]
---
# Limit history --all to the queue named with --queue

`asqr history --queue <name> --all` lists the finished sessions of
every queue, so `--queue` has no effect next to `--all`. When a queue
is named, `--all` should keep to that queue and list all of its
entries; only `--all` without `--queue` (or `--dir`) lists every queue.
The help text of `--all` changes with it.

## Problem

Found on 2026-11-04 with asqr 0.17.2:

```text
$ asqr history --queue timecapsule --all --json | jq -r '.[] | .queue'
timecapsule
asqr
asqr-todo
asqr-todos
dachser
default
ntropy
```

`--all` is documented as "List the finished sessions of every queue
under the data directory", which matches what it does but not what a
person expects after naming a queue: they read `--all` as "all entries"
of the queue they named.

## Suggested change

- `--all` alone: every queue, as today.
- `--queue <name> --all` or `--dir <path> --all`: only that queue, with
  every entry. Whether `--all` then also lifts `--limit` (default 10)
  is to settle while implementing; "all entries" suggests it does.
- The help text of `--all` says both readings.
