# 38. Give notices kinds and confirm a finished session

Date: 2026-10-15

## Status

Accepted

Amends [37. Colour the TUI by roles](0037-colour-the-tui-by-roles.md)

## Context

The line above the key bar shows a notice about the queue or a message
about the last key until the next key or paste. Each was plain text, and
ADR 37 drew all of them in the warning colour, yellow. A draft that
could not be saved looked the same as a refused key or a session file
that was reloaded.

The design guide's terminal section gives a key that was refused the
warning colour (yellow), a failure such as an inbox that cannot be read
the error colour (red), and a submitted session the success colour
(green).

The user decided on 2026-10-15 that notices get kinds (information,
warning, error, success) before asqr confirms a submitted or rejected
session, and that one ADR covers both.

## Decision

Every notice and every message carries one of four kinds, and the status
line draws it in the kind's colour. The rule for the kind was settled in
the review of the implementation plan on 2026-10-15:

| Kind | When | Colour |
|---|---|---|
| Error | an operation failed and nothing was done | red |
| Warning | asqr refused an action or set something aside on purpose | yellow |
| Info | plain news | the text colour |
| Success | a session was sent | green |

By that rule:

- **Error:** an inbox that cannot be read, a session file that cannot be
  read, a file that cannot be archived, an invalid session that cannot
  be answered with an error, a draft that cannot be saved, a session
  that cannot be finished, an image that cannot be opened.
- **Warning:** a session archived unanswered because an unread result
  with its id is in the outbox, an invalid session answered with an
  error or archived without a result, a submit that was not sent
  because the file changed on disk, and every refused key or paste
  (`o` without an image, a pick past `max`, `n` on a question without
  notes, a paste where no field takes it).
- **Info:** a session file that changed on disk and was reloaded, and
  a session that was rejected.
- **Success:** a session that was sent.

When a submit finds the file changed and the inbox then cannot be read,
the error replaces the warning, as the status line holds one notice.

### Confirming a finished session

A submitted or rejected session leaves the screen at once, and the next
session or the empty queue takes its place. Once the result is written,
a notice says so, with the session's title, or its id when it has none:

    ✔ sent "Release 0.2.0" · 1 answered, 2 skipped
    ✗ rejected "Release 0.2.0"

The user chose this format on 2026-10-15, with the sent line in the
success colour and the rejected line neutral, so the sent line is a
success and the rejected line information. The counts are the review's
answered and skipped questions. A submit or reject that does not finish
shows its error or warning instead.

The user decided on 2026-10-15 that the confirmation stays until the
next key or paste, also over the next session and over the empty queue.
A notice is cleared by a key or a paste alone, so reading the inbox
again after the finish keeps it.

## Consequences

- The warning colour no longer marks every notice: ADR 37's table gave
  it "a notice"; it now marks refusals and sessions set aside.
- Under `NO_COLOR` the kinds look alike; only the text tells them apart.
