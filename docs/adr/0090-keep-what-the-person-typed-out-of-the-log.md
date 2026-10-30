# 90. Keep what the person typed out of the log

Date: 2026-10-30

## Status

Accepted

Amends [15. Configure through flags and environment and log into the cache directory](0015-configure-through-flags-and-environment-and-log-into-the-cache-directory.md):
the log holds no text the person typed

## Context

- The `replaced a session` line of `ask --replaces` logged the withdrawn
  session's result in its `Debug` form. That result carries the note,
  the answers with their custom text, and the draft of the person.
- The error of a draft that cannot be read came from `serde_json`, which
  quotes the value it did not expect, as in `invalid type: string
  "<text>", expected a boolean`. The TUI logs that error, and a draft
  holds what the person typed.

## Decision

- The log holds no text the person typed.
- A line about a session or its result names ids, status and counts,
  such as the number of answers, never answers, notes, custom text or
  draft content.
- The `replaced a session` line logs the new and the old id, whether the
  old session was withdrawn now or before, the status of its result, the
  number of answers in the result and the number of answers in its
  draft.
- The error of a draft that cannot be read names the kind of error and
  its line and column, not the value.

Rejected: keeping the full dump of the result in the line.

## Consequences

Logs written by earlier versions may hold text the person typed.
