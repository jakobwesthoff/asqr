---
title: "Accept a session ULID wherever a reference is taken"
kind: improvement
component: cli
status: needs-discussion
origin: request
tags: [ux]
---
# Accept a session ULID wherever a reference is taken

`asqr ask` prints a session's ULID, and agents hand that ULID to the
person, but `asqr replay` only takes the short reference that `asqr
history` prints. Check whether `replay`, and every other command that
takes a reference, can also accept the full ULID as an extra form, with
the reference working as before. If nothing speaks against it, add it
for convenience.

## Problem

Found on 2026-11-04 with asqr 0.17.2. An agent gave the person the
session's ULID to look the session up again:

```text
$ asqr replay 01M6KN9EE9TNBA3YM4XYS7Z2KB --json
error: "01M6KN9EE9TNBA3YM4XYS7Z2KB" is too long; a REF has at most 8 characters
```

The person then had to run `asqr history` to find the reference
(`6ERF18MP` for that session). `asqr history --json` lists both `id`
(the ULID) and `ref` per entry, so the mapping exists. Elsewhere the
ULID is the handle: `asqr ask` prints it, and `asqr wait`, `asqr
result` and `asqr withdraw` take it.

## To check

- Whether a 26-character ULID can be told apart from a reference
  without ambiguity (a reference is at most 8 characters, so by length
  alone it seems so).
- Which commands take a reference (`replay` at least) and whether all
  of them can resolve a ULID through the same lookup `history` uses.
- Whether the error text should mention that a ULID works too.

## Decision wanted

If the check finds no problem, accept the ULID as an additional form;
the reference stays as it is.
