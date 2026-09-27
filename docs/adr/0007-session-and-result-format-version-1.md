# 7. Session and result format version 1

Date: 2026-09-27

## Status

Accepted

## Context

The file format is what other tools build on, so it has to be small,
versioned and checkable.

## Decision

The format is JSON, versioned with `"asqr": 1`. The full description is
in `docs/spec.md` sections 4 and 5.

- Question kinds are `single`, `multi` (with optional `min` and `max`)
  and `text`. Other kinds such as `confirm`, `rank` and `number` are left
  out of version 1.
- Options can be any number, each with a label and an optional long
  description.
- Optional per question:
  - custom entry
  - one note per question
  - a length limit (target, warn, max) with a live counter
  - an image
  - `required`
- Text fields use a small Markdown subset: bold, italic, inline code,
  lists and line breaks.
- There is one result per session, written on submit, with the status
  `submitted`, `cancelled` or `error`. There are no partial submits.
- `asqr ask` rewrites relative image paths to absolute ones. Files
  dropped into the inbox by hand must use absolute paths, and `asqr
  validate` warns about relative ones.
- A JSON Schema is derived from the Rust types. `asqr schema` prints it
  and `asqr validate` checks a file.

## Consequences

Adding kinds or fields later needs format version 2 or optional fields
that version 1 readers ignore.
