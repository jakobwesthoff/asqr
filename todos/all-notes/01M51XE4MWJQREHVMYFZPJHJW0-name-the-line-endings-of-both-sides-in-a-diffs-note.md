---
title: "Name the line endings of both sides in a diff's note"
kind: improvement
component: tui
impact: low
origin: request
tags: [ux, field-test]
---
# Name the line endings of both sides in a diff's note

Raised on 2026-10-16 while Jakob tested a diff of a CRLF file against an
LF file. A diff compares lines without their endings and says
`line endings differ` (`src/tui/render/diff.rs`, above the hunks and
also below "no changes"), but not which ending each side uses.

## Decisions (Jakob, 2026-10-16, asqr session 01M51ZBV7A04CN2RED495S89V6)

- The note names both sides by name: `line endings differ: old CRLF,
  new LF`.
- A side with several kinds of endings is named with them:
  `old mixed CRLF and LF`. In a narrow pane the note is cut at the
  edge, as notes are.

## Notes for the change

- `Note` holds a `&'static str` (`src/tui/render/source_line.rs:282`);
  this note needs owned text, such as `Cow<'static, str>`.
- Tests: CRLF against LF, mixed endings, a lone CR.
