---
title: "Show text entries in the context pane with syntax highlighting"
kind: feature
component: tui
status: blocked
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

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
- Depends on: [Make the context area a pane of its own with focus and scrolling](01M4XHQSC7Y60Y5PPZ3W52YAXW-make-the-context-area-a-pane-of-its-own-with-focus-and-scrolling.md)
- Depends on: patine todo 01M4XHNXXNJNV57SNBYH88EFAN (Render to style-neutral lines so other tools can share the renderer)
