---
title: "Share the row drawing of the code, diff and Markdown viewers"
kind: refactor
component: tui
origin: review
---
# Share the row drawing of the code, diff and Markdown viewers

Found in a refactor review of the TUI's rendering on 2026-10-15. The
three text viewers of the context pane repeat the same pieces, each in
its own way (line numbers at `a3768f6`, all under `src/tui/render/`):

- The text area right of a fixed gutter, built as the same `Rect` in
  `code.rs` l.158 and l.172-177 and `diff.rs` l.282-288; the row loop over
  the area from the offset (`diff.rs` l.255, `markdown_viewer.rs` l.142,
  a variant in `code.rs` l.159); the dim gutter (`code.rs` l.171,
  `diff.rs` l.274).
- The notes of a file cut at 8 MiB (`CUT_NOTE`, `END_NOTE`,
  `source_file.rs` l.23, l.27): `code.rs` computes virtual rows in `draw`
  and cuts them at the width (l.161, l.166), `diff.rs` pushes them as rows
  and cuts them (l.147, l.175, l.216), `markdown_viewer.rs` wraps them
  (l.117, l.123). One rule of the spec, three behaviours.
- `enum Content { body, Note(Placeholder) }` with the same `content()`
  accessor and `expect` in all three (`code.rs` l.45, `diff.rs` l.60,
  `markdown_viewer.rs` l.57, l.93).

## Proposal

- A one-row helper in `source_line.rs`, such as a `GutterRow` widget or
  `draw_after_gutter(buf, row, gutter, left)`, plus one way to walk the
  rows from an offset.
- One cut-notes rule in `source_file.rs` used by all three viewers, cut
  at the width as two of them do today.
- `enum Loaded<T> { Body(T), Note(Placeholder) }` with
  `impl<T: Viewer> Viewer for Loaded<T>` in `viewer.rs`.

## Done when

- The three viewers draw gutter rows, cut notes and placeholders through
  the shared pieces; their buffer tests and snapshots pass unchanged,
  except the Markdown cut notes on panes narrower than their text, which
  no longer wrap.

## Relations

- Relates to: [Highlight the syntax of a diff by its language](01M4ZRQN3BQWXQXQJ1ZMH23Q1N-highlight-the-syntax-of-a-diff-by-its-language.md), which changes the same diff drawing
