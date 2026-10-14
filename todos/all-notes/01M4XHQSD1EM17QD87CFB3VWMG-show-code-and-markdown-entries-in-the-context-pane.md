---
title: "Show code and Markdown entries in the context pane"
kind: feature
component: tui
horizon: next
origin: discussion
tags: [ux, milestone/v0-11]
---
# Show code and Markdown entries in the context pane

Step 4 of the context pane plan. On 2026-10-14 the user replaced the
single `text` entry (one viewer with a `render` flag) by two entry
types with a viewer each: `code` for verbatim content with highlighting,
and `markdown` for rendered Markdown.

## Why two types

- Each viewer does one job. The code viewer reads, highlights once,
  shows line numbers, cuts long lines and scrolls sideways; the Markdown
  viewer renders per width, wraps, and has neither gutter nor sideways
  scrolling. In one viewer these were two paths behind a flag, with
  different caching.
- No flag that only means something for one language: `render: false`
  goes away. A `.md` file as a `code` entry shows its source, which is
  what reviewing the Markdown itself needs.
- `text` was overloaded: the question kind `text`, the question field
  `text` and the inline content key `text` already exist, so an agent
  could easily put an explanation into a `text` entry that belongs in
  the question. `code` maps onto the fenced code block agents write all
  the time (verbatim, monospace, a language tag), which is also where
  logs, terminal output and config files go.
- For the same reason the inline content key `text` becomes `content`
  in every entry, diff sides included, so "text" and "kind" each mean
  one thing in the format.

## Proposal

1. **Format commit first.** Format version 2 is unreleased, so the
   version stays 2:
   - `{ "type": "code", "title"?, "path" | "content", "language"? }`
   - `{ "type": "markdown", "title"?, "path" | "content" }`
   - diff sides become `{ "path" }` or `{ "content" }`.
   - `type: "text"` and `render` are gone; the inline key `text` is
     gone everywhere.
   - Types, validation, schema, the skill (when to use `code` and when
     `markdown`, the `content` key), spec sections 4 and 7.7, an
     amendment of ADR 28, the changelog, `asqr new`, and the example
     sessions.
2. **Code viewer.** Highlighting through patine 1.5.0's
   `highlight(code, language)` (`syntect`, `two-face` grammars, the
   8-colour ANSI theme) in `load`, once. The language comes from
   `language`, else from the extension of `path` or `title`; failing
   that, plain text. A dim gutter with line numbers on every code entry.
   Lines are drawn through `SourceLine` (`src/tui/render/source_line.rs`,
   ADR 33): tabs to stops of 4, control pictures, widths in columns, the
   column window with the dim `›` at a cut. The viewer reports the full
   width of its widest line, so the pane scrolls sideways.
3. **Markdown viewer.** patine's `render_lines` at the body's width in
   `lay_out` (so it runs only when the size changes), wrapped, no
   gutter. The adapter from patine's `TextStyle` to ratatui's `Style`
   (double underline becomes single; `AnsiColor` via
   `Color::Indexed(color.index())`, so index 7 stays patine's white) is
   shared with the full Markdown todo; whichever lands first builds it.
   patine passes tabs inside code blocks through verbatim; draw them
   through `SourceLine`'s rules as well.
4. A file that cannot be read shows through `Placeholder` in
   `src/tui/render/viewer.rs`, as in the diff viewer.
5. Remove the placeholder for text entries in `viewer_for` with its
   TODO, and every statement that text entries show a placeholder
   (skill, spec 7.7, ADR 28's bullet, the changelog).

## Where the pane leaves it

The context pane (ADR 31) shows every entry through the `Viewer` trait
in `src/tui/render/viewer.rs`: `load` once for everything that does not
depend on the body's size (reading the file, highlighting), with a
failure shown as content; `lay_out` for each new body size, returning
the lines and the widest line; `draw` into a buffer at a scroll offset.
The pane scrolls sideways when the reported width exceeds the body's.
`Viewers` keeps a code or Markdown viewer, and what it derived, while
its session waits, also when the terminal regains the focus.

## Relations

- Part of: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md)
