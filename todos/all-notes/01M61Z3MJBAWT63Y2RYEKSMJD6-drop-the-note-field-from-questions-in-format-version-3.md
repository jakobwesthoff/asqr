---
title: "Drop the note field from questions in format version 3"
kind: refactor
component: format
horizon: someday
origin: request
---
# Drop the note field from questions in format version 3

Every question and every info item takes a note (decided by the user on
2026-10-28; ADR 68, "Every item takes a note"). Format version 2 keeps reading `"note": false` only so that
released session files stay valid: the field has no effect, and
`validate` and `ask` warn that it is ignored. The user asked on
2026-10-28 that a format version 3, whenever there is one, drops the
field completely.

## What to do in version 3

- Remove `note` from the question (and info item) shape, the schema in
  `schema/`, the spec's field list and the skill.
- Remove the "note: false is ignored" warning and the parsing that only
  exists to produce it.
- Decide how a version 3 file that still sets `note` is treated, in line
  with the version 3 rule for unknown fields.

## Revisit when

A format version 3 is planned.
