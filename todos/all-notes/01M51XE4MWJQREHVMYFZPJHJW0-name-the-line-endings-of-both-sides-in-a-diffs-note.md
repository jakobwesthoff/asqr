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
`line endings differ` above its lines (`src/tui/render/diff.rs`), but
not which ending each side uses, so the person cannot tell what to fix.

## Proposal

Name both sides, such as `line endings differ: old \r\n, new \n`, and
`mixed` for a side that uses several. Marking each line with `␍` was
considered and is too loud.
