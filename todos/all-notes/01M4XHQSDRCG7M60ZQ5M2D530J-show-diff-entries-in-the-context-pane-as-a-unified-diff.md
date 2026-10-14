---
title: "Show diff entries in the context pane as a unified diff"
kind: feature
component: tui
status: blocked
horizon: next
origin: discussion
tags: [ux, milestone/v0-11]
---
# Show diff entries in the context pane as a unified diff

Step 5 of the context pane plan (design settled with the user on
2026-10-14). A `diff` entry carries old and new text; asqr computes the
diff and shows it as a unified diff in the context pane.

## Proposal

The plan's Design section, *Viewers*, has the full design. In short:

- Hunks with 3 context lines, a dim separator line between hunks.
- Old and new line numbers in a dim two-column gutter.
- Removed lines red, added lines green (palette colours); inside a
  changed line, the changed words are emphasised on top.
- A missing `old` shows a new file, a missing `new` a deleted one.
  `name` goes into the header.
- Long lines scroll sideways like code in the text viewer. No syntax
  highlighting inside the diff and no whole-file view in this phase.

## Decisions

- 2026-10-14, user: the diff crate is `similar` 3.x (by the author of
  `insta`, which asqr already uses); its `inline` feature gives the word
  emphasis.

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
- Depends on: [Make the context area a pane of its own with focus and scrolling](01M4XHQSC7Y60Y5PPZ3W52YAXW-make-the-context-area-a-pane-of-its-own-with-focus-and-scrolling.md)
