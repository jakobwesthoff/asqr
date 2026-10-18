# 29. Let the person add a note to the whole session

Date: 2026-10-14

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md)

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md)

Amended by [36. Treat every typed field alike](0036-treat-every-typed-field-alike.md):
the session note takes several lines

## Context

Every question takes a note, but the session as a whole did not. A
note on the review tab lets a submitted session carry information that
belongs to no single question: an approval with a condition, or a remark
that spans several questions.

## Decision

- **A live field:** the note is typed in place on the review tab, like
  the reject reason, and behaves like every typed field (ADR 36). There
  is no `n` key on the review tab.
- **Its own row:** the note is a row below Submit and above Reject. The
  review opens on Submit (ADR 8), and a field there would take every
  key on arrival, so `q`, `?`, `L` and the vim keys would type into the
  note instead of acting. Submit stays a row whose keys are commands.
- **Keys:** `enter` in the note submits with it. `↑`/`↓` leave the row
  from its first or last row, `esc` stops typing, and `←` at the start
  of the text goes to the last question, as on the reject row.
- **Result:** a submitted result carries the note as a top-level `note`,
  trimmed and absent when blank, like `reason`. A rejected result
  carries `reason` and never a `note`. The field belongs to result
  format version 2.
- **Draft:** the draft keeps the note as typed in a top-level `note`. A
  note that is not blank counts as an answer, so `ask` refuses to
  replace the session and `status --json` reports
  `draft_has_answers: true`. The reject reason does not count.

## Consequences

- An agent reading a submitted result has to look at `note` next to the
  answers. The skill says so.
- Reaching Reject from Submit takes two steps down, through the note.
- A typed session note keeps `ask` from replacing the session, so the
  person does not lose it.
