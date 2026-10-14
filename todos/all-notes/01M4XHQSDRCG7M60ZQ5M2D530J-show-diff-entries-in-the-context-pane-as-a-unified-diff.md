---
title: "Show diff entries in the context pane as a unified diff"
kind: feature
component: tui
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

## Where the pane leaves it

The context pane (ADR 31) shows every entry through the `Viewer` trait
in `src/tui/render/viewer.rs`: `load` once for everything that does not
depend on the body's size (reading the file, highlighting, diffing), with
a failure shown as content; `lay_out` for each new body size, returning
the lines and the widest line; `draw` into a buffer at a scroll offset.
The pane scrolls sideways when the reported width exceeds the body's, so
a viewer that cuts lines reports their full width. `viewer_for` there
still builds the placeholder for this kind of entry, with a TODO naming
this todo. `Viewers` keeps a text or diff viewer, and what it derived,
while its session waits, also when the terminal regains the focus.

## Decisions

- 2026-10-14, user: the diff crate is `similar` 3.x (by the author of
  `insta`, which asqr already uses); its `inline` feature gives the word
  emphasis.

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
