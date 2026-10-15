---
title: "Highlight the syntax of a diff by its language"
kind: feature
component: tui
status: needs-discussion
origin: discussion
tags: [ux]
---
# Highlight the syntax of a diff by its language

Agreed with the user on 2026-10-15 after a merge request review run through
asqr. Code entries are highlighted, diff entries are not (ADR 33, "no
syntax highlighting inside the diff ... in this phase"). In a review, most
of what the person decides on is a suggested code change, so the diff is
the entry they read most, and it is the one without highlighting.

## What happened

In the review of a TypeScript/React merge request, every suggested change
went to the person as a diff entry against the head file, and every one
showed as plain red and green text. For the final preview the asking agent
turned the suggestions into highlighted Markdown code blocks instead, only
to get the colours back, and lost the diff view with it: the person saw
the new code, but no longer what it replaced.

## Proposal

Highlight the lines of a diff entry in the file's language, as code
entries are, while keeping what ADR 33 decided: the red and green per
line, the reversed changed words, the signs and the gutter.

## Open questions

- How highlighting and the line colours combine: a tinted background per
  line with highlighted foreground, or highlighting only on context lines.
  The palette rule of ADR 33 (the terminal's own colours) has to hold.
- Where the language comes from: the extension of `name` like for code
  entries, and whether diff entries get the `language` key the spec
  currently rules out ("a diff shows without syntax highlighting, so it
  takes no language"), which is a format change.
- Whether a hunk is highlighted on its own or with the whole old and new
  text, so that a hunk starting inside a block comment or a template
  string is coloured right.

## Relations

- Relates to: [Show diffs side by side in a wide context pane](01M4XHD347ZCN7QTYARPK73WBV-show-diffs-side-by-side-in-a-wide-context-pane.md)
