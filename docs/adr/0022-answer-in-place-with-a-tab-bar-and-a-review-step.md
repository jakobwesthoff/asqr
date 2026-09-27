# 22. Answer in place with a tab bar and a review step

Date: 2026-09-27

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md)

## Context

The user looked at the first screens and the key model of ADR 8 and
asked for a simpler interaction:

- the own answer as a row like the options, typed in place
- no separate submit key, since the last step should be the submission
- `enter`, not `space`, to choose on a single-choice question
- a TUI that adapts to the terminal size

Two rounds with the question tool settled the details. The user's
wording is in the logbook.

## Decision

- **Layout:** one full-width column. A tab bar lists the questions,
  marked `☒` or `☐`, with a review tab last. It replaces the question
  list.
- **Navigation:** `←`/`→` and `h`/`l` switch questions. `↑`/`↓` and
  `j`/`k` move between rows.
- **Picking:** on `single`, `enter` or a digit picks and moves on to the
  next question. On `multi`, `space` or a digit toggles and `enter` moves
  on.
- **Live fields:** the own answer is the last row and a live field,
  typed in its line without a frame. It is always one line and scrolls
  sideways. On `multi` it counts once it has text.
- **Text answers and notes:** a `text` answer and the note (`n`) are
  edited in place and can have several lines, added with `ctrl-j`.
- **Inside a field:** `←`/`→` move the text cursor, `↑`/`↓` leave the
  row, and `esc` leaves the field while the cursor stays.
- **Review tab:** it lists all answers, marks missing required ones, and
  holds Submit and Reject, the latter with a live reason field.
- **Removed keys:** `S`, `X`, `c`, `tab` and `shift-tab`.
- **No hard limit:** `length.max` is dropped. The counter guides with
  `!` above the target and `!!` above `warn`, and input is never
  refused.
- **Format:** `custom.multiline` is dropped, since own answers are
  always one line.

Rejected in the rounds:

- an own answer that needs `enter` before typing
- typing that selects the own answer on single-choice questions
- the review step with an extra shortcut, and submitting straight after
  the last answer without a review
- notes as a separate row
- keeping the question list next to the tab bar

## Consequences

- The TUI state of phase 4 and the drawing of phase 5 are reworked.
- An asker can no longer enforce a length, only guide it.
- Format version 1 changes before anything was released. `max` and
  `multiline` in existing files become unknown fields, which parse and
  produce a warning.
