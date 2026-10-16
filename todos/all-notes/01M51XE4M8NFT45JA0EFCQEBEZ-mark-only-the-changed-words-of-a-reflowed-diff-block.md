---
title: "Mark only the changed words of a reflowed diff block"
kind: improvement
component: tui
origin: request
tags: [ux, field-test]
---
# Mark only the changed words of a reflowed diff block

Raised by Jakob on 2026-10-16 while testing the diff of
`src/tui/render/viewer.rs` before and after ADR 46: the marked words of
a changed block did not show what changed.

## Problem

`src/tui/render/diff.rs` hands each replaced block to similar's
`iter_inline_changes_with_options_deadline` with semantic cleanup. It
diffs all removed lines of the block against all added lines as one
stream of words, in which line breaks, indentation and comment leaders
such as `///` are tokens like any other. In a comment that was
reflowed after an edit this marks:

- the `///` and the indentation of lines whose break moved,
- words that only moved to another line (`shown and`),
- whole lines that are entirely new, which the line colour already
  says.

The words that really changed (`calls \`load\` once,`, `and loaded`)
drown in that.

## Proposal

- Compare a block's words across its lines as now, but treat a line
  break together with the following line's indentation and comment
  leader (`///`, `//`, `#`, `*`, `>`, `--`) as white space: never a
  token of its own, never marked.
- Do not mark a line whose every word would be marked. The test
  `a_line_rewritten_entirely_has_no_emphasis` covers this for a single
  line, not inside a block of several.

Settle the details in a plan with an adversarial review before the
change.
