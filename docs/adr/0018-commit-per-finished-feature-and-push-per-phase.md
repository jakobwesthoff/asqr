# 18. Commit per finished feature and push per phase

Date: 2026-09-27

## Status

Accepted

## Context

Development is test first (ADR 11), and the pre-commit hook refuses
commits that do not pass `just check`.

## Decision

- Each finished feature is one commit, with its tests. The red phase of
  each cycle is shown while working and does not land in the history.
- Commits are pushed at the end of each phase of the version 1 plan
  (`docs/plans/`).

Rejected: a commit per red-green cycle, separate commits for the red
test and the fix (the hook would have to be bypassed), and pushing after
every commit.

## Consequences

Every commit in the history passes the pipeline. CI runs once per
finished phase, not per commit.
