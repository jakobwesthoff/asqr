---
title: "A side cut at 8 MiB between CR and LF names a CR line end it does not have"
kind: bug
component: tui
impact: low
origin: review
tags: [ux, field-test]
---
# A side cut at 8 MiB between CR and LF names a CR line end it does not have

Found in the adversarial review of the line-endings note (2026-10-16,
commit `b9d56e2`). asqr reads at most the first 8 MiB of a diff side.
When that cut falls between the `\r` and the `\n` of a CRLF line end,
the read text ends in a lone `\r`, and the note names a `CR` end (or
`mixed CRLF and CR`) that the file does not use. The bare note before
`b9d56e2` was wrong in the same case. Read in the code, not reproduced.

## Proposal

When a side is cut, drop a trailing lone `\r` before the line ends are
counted, or cut before a `\r` that would be split from its `\n`. Test
with a side whose CRLF straddles the 8 MiB boundary.
