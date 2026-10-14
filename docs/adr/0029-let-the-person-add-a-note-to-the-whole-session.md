# 29. Let the person add a note to the whole session

Date: 2026-10-14

## Status

Accepted

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

Amends [28. Session and result format version 2 with a context field](0028-session-and-result-format-version-2-with-a-context-field.md)

One-line session note superseded by [36. Treat every typed field alike](0036-treat-every-typed-field-alike.md)

## Context

Every question takes a note, but the session as a whole did not. The
user asked on 2026-10-14 for a note on the review tab, so that a
submitted session can carry information that belongs to no single
question: an approval with a condition, or a remark that spans several
questions. ADR 28 named this note as the first new field of result
format version 2.

## Decision

- **A live field:** the note is typed in place on the review tab, like
  the reject reason. It is one line and wraps like the reject reason
  (ADR 27). There is no `n` key on the review tab (user, 2026-10-14).
- **Its own row:** the note is a row below Submit and above Reject. The
  review opens on Submit (ADR 23), and a field there would take every
  key on arrival, so `q`, `?`, `L` and the vim keys would type into the
  note instead of acting. Submit stays a row whose keys are commands.
- **Keys:** `enter` in the note submits with it. `↑`/`↓` leave the row,
  `esc` stops typing, and `←` at the start of the text goes to the last
  question, as on the reject row.
- **Result:** a submitted result carries the note as a top-level `note`,
  trimmed and absent when blank, like `reason`. A rejected result
  carries `reason` as before and never a `note` (user, 2026-10-14).
- **Format version:** the field belongs to result format version 2,
  which ships with it in 0.11.0 (user, 2026-10-14).
- **Draft:** the draft keeps the note as typed in a top-level `note`. A
  note that is not blank counts as an answer, so `ask` refuses to
  replace the session and `status --json` reports
  `draft_has_answers: true`. The reject reason still does not count.

## Consequences

- An agent reading a submitted result has to look at `note` next to the
  answers. The skill says so.
- Reaching Reject from Submit takes two steps down, through the note.
- A person can no longer lose a typed session note to a replacing
  `ask`.
