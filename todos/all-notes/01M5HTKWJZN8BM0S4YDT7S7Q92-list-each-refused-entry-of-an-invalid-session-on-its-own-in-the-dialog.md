---
title: "List each refused entry of an invalid session on its own in the dialog"
kind: bug
component: tui
impact: low
origin: implementation
tags: [ux]
---
# List each refused entry of an invalid session on its own in the dialog

Found on 2026-10-22 while making unknown context types explain
themselves. The dialog of an invalid session lists each validation error
on its own line, but the problems the parser refuses before validation
(another format version, a `type: "text"` entry, an unknown context
type) arrive as one item, joined with "; ".

## Problem

`Inbox::load` in `src/tui/inbox.rs` (around line 172) maps a
`ParseError` with `vec![error.to_string()]`, and
`ParseError::Refused(Vec<ValidationError>)` in `src/format/session.rs`
displays its list joined with "; ". Validation errors go through
`errors.iter().map(ToString::to_string)` and stay apart. Read in the
code (2026-10-22).

## Suggested fix

Keep a `Refused` list apart up to the dialog, as validation errors are:
one dialog item per refused entry. The error result and the log keep
joining them, as they do for validation errors.

## Done when

- A session with two refused entries shows two items in the dialog, in
  a test.
