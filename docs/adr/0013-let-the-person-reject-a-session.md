# 13. Let the person reject a session

Date: 2026-09-27

## Status

Accepted

Amended by [53. Keep the notes and answers in the result of a rejected session](0053-keep-the-notes-and-answers-in-the-result-of-a-rejected-session.md):
the result keeps the session note and the answers

## Context

Sometimes a whole session is wrong: out of date, badly asked, or not
the person's business. Skipping every question does not tell the asker
that.

## Decision

The review tab holds Reject, a row with a live field for an optional
reason (ADR 8). `enter` there rejects the whole session. The result has
the status `cancelled` and the reason in `reason`.

Rejected: rejecting without a reason, and having no way to reject.

## Consequences

Askers have to handle `cancelled` and should show the reason. `asqr ask
--wait` exits with 10 for it.
