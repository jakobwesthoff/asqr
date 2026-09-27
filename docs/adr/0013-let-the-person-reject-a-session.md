# 13. Let the person reject a session

Date: 2026-09-27

## Status

Accepted

## Context

Sometimes a whole session is wrong: out of date, badly asked, or not
the person's business. Skipping every question does not tell the asker
that.

## Decision

`X` rejects the whole session after a confirmation. The person may give
a reason. The result has the status `cancelled` and the reason in
`reason`.

Rejected: rejecting without a reason, and having no way to reject.

## Consequences

Askers have to handle `cancelled` and should show the reason. `asqr ask
--wait` exits with 1 for it.
