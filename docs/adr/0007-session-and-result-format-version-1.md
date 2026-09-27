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

## Amendment (2026-09-27, adversarial review F4, F5, F10, F11)

- **Answer state** (spec section 5.2):
  - A default counts as answered, and the answer carries `defaulted:
    true` until the person edits the question.
  - A note never answers a question and never satisfies `required`.
  - `min` and `max` only apply once something is selected.
  - Custom text that is empty after trimming does not answer the
    question.
  - The answer shape per kind is fixed.
- **Drafts** use the result format with `status: "draft"` and `current`.
- **Results** carry `session_sha256` over the session file's bytes.
- **Ids:** the file stem is the id, and an `id` field must match it. No
  leading `.`, at most 200 bytes, and ids are compared
  case-insensitively on every platform.
- **Unknown fields** are ignored when parsing and reported as warnings.
  `"asqr": 2` is the validation error "unsupported format version".
- **`follows`** is only shown in the header.

## Amendment (2026-09-27, UX review, ADR 22)

`length.max` and `custom.multiline` are dropped from format version 1
before any release. Lengths only guide (`target`, `warn`), and own
answers are always one line. In files that still use them they are
unknown fields and produce a warning.
