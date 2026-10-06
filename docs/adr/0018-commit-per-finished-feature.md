# 18. Commit per finished feature

Date: 2026-09-27

## Status

Accepted

## Context

Development is test first (ADR 11), and the pre-commit hook refuses
commits that do not pass `just check`.

## Decision

Each finished feature is one commit, with its tests. The red phase of
each cycle is shown while working and does not land in the history.

Rejected: a commit per red-green cycle, and separate commits for the
red test and the fix (the hook would have to be bypassed).

## Consequences

Every commit in the history passes the pipeline.
