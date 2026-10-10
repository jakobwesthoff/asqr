---
title: "ctrl-g: edit the focused field in $EDITOR"
kind: feature
component: tui
status: needs-discussion
origin: request
tags: [ux]
---
# ctrl-g: edit the focused field in $EDITOR

The user requested this on 2026-09-27 during phase 6. It is not scheduled yet.

## Wish

In every input field (own answer, text answer, note, reject reason),
`ctrl-g` writes the field's content into a temp file, opens it in the
person's editor (`$VISUAL`, then `$EDITOR`, e.g. vim), and after the
editor exits takes the file's content back into the field. Longer
answers and notes can then be written with a real editor.

## Existing pieces

- The `edit` crate (0.1.5, "Open a file in the default text editor")
  picks the editor from `VISUAL`/`EDITOR` with platform fallbacks.
- The yaw project (`~/Development/github/jakobwesthoff/yaw`,
  `src/editor.rs`) already does this: it writes a `tempfile::NamedTempFile`
  with a suffix and calls `edit::edit_file(path)`, then reads the file
  back.

## Things to settle when implementing

- The TUI owns the terminal: before the editor starts, leave the
  alternate screen and raw mode (`ratatui::restore`), and set both up
  again afterwards (`ratatui::init`), then redraw everything. The
  key-reading thread in `src/terminal.rs` must not steal the editor's
  input while it runs; it probably has to pause, since it blocks in
  `crossterm::event::read`.
- This is terminal work, so it goes through the `Host` trait
  (`src/tui/run.rs`), like opening an image: a new `Effect`/`AppEffect`
  asks for it, the host runs the editor, the loop puts the text back.
- One-line fields (the own answer) take a single line: decide whether
  newlines from the editor are joined with spaces or refused with a
  message (ADR 22 keeps own answers to one line).
- The temp file suffix (`.md`, `.txt`) decides the editor's syntax
  mode; the question's text could go in as a comment header, but then
  it has to be stripped reliably on the way back.
- Editor exits with an error, or the file is unchanged: keep the field
  as it was.
- The key goes into the key bar and help for focused fields, and into
  spec section 7.2 and an ADR amendment of ADR 22.
