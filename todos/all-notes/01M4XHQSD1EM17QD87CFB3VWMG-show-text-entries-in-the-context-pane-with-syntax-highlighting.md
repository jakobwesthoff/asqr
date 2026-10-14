---
title: "Show text entries in the context pane with syntax highlighting"
kind: feature
component: tui
horizon: next
origin: discussion
tags: [ux, milestone/v0-11]
---
# Show text entries in the context pane with syntax highlighting

Step 4 of the context pane plan (design settled with the user on
2026-10-14). A `text` entry shows source code, plain text or Markdown
in the context pane: code highlighted like patine, Markdown rendered by
patine's renderer.

## Proposal

The plan's Design section, *Viewers*, has the full design. In short:

- Highlighting through patine's shared `highlight(code, language)`
  (`syntect`, `two-face` grammars, the 8-colour ANSI theme), drawn
  through the same adapter to ratatui styles as question texts.
- The language comes from `language`, else from the extension of `path`
  or `title`; failing that, plain text.
- Source views (code, plain text, Markdown with `render: false`) get a
  dim gutter with line numbers and scroll sideways, with a dim `›`
  marking a cut line. Rendered Markdown wraps and has no gutter.
- Markdown renders through patine's `render_lines` at the pane's width,
  unless the entry says `render: false`.
- A file is read when the entry is first shown; read errors show in the
  pane like a missing image does today.

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

The diff viewer (ADR 33, `src/tui/render/diff.rs`) draws its lines
through `SourceLine` in `src/tui/render/source_line.rs`: tabs expanded
to stops of 4 columns, control characters shown as their pictures,
widths in terminal columns, and a column window that turns a wide
character cut by an edge into blanks and ends a line cut at the right
in a dim `›`. ADR 33 makes these rules hold for every viewer of source
lines, so this viewer draws through the same module; highlighted runs
become its styled pieces. The line number gutter is each viewer's own.
A file that cannot be read shows through `Placeholder` in `viewer.rs`,
as in the diff viewer.

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
