---
title: "Full Markdown in question texts, rendered like patine"
kind: feature
component: tui
status: needs-discussion
origin: request
tags: [ux]
---
# Full Markdown in question texts, rendered like patine

The user requested this on 2026-09-28. It is not designed yet.

## The request

Questions should be formattable with Markdown, with syntax highlighting,
rendered in an unobtrusive way like the user's tool patine (source in
`~/Development/github/jakobwesthoff/patine`, version 1.4.0 at `b5411d3`).

## Where asqr stands

Text fields (`intro`, question `text`, option `description`) support a
small subset: bold, italic, inline code, lists and line breaks. Every
other element is shown exactly as written in the source (spec sections 4
and 7.6, `src/tui/render/markdown.rs`, parsed with `pulldown-cmark`).

## How patine renders (from its README and source)

- Less color, more structure: the terminal's default colors, bold,
  italic, underline and dim. Where it colors, only the 8-color ANSI
  palette, so the user's terminal theme applies.
- Headings: H1 italic and underlined, H2 to H6 bold.
- Inline: bold, italic, bold-italic, strikethrough, inline code.
- Code blocks: verbatim, never wrapped, syntax highlighted when the
  fence names a language: `syntect` through `two-face` (bat's grammars),
  restricted to the `Ansi` theme (palette indices 0-7). Rationale in
  patine's `docs/adr/0005-syntax-highlighting-via-syntect-with-ansi-theme.md`;
  code in `src/highlight.rs` (lazy `SyntaxSet` and theme singletons).
- Links: underlined text, URL in dimmed parentheses. Images:
  `[image: alt text]` with the path dimmed.
- Lists with nesting; tables with box-drawing borders and bold centered
  headers; blockquotes with a `│` bar, dimmed italic; horizontal rules
  as `─` across the width; word wrapping at the given width.
- Parser: the `markdown` crate (mdast, GFM plus frontmatter), patine's
  ADR 0002. Output is written straight to a `Write` with crossterm
  styles (ADR 0003), not ratatui spans.

## Open points to decide

- Reuse or port: patine renders to a writer with escape sequences, asqr
  draws ratatui spans into a layout that rewraps on every resize. Options:
  port patine's rules into asqr's renderer; extract a shared crate from
  patine that yields styled lines; or render with patine and convert the
  escape sequences (for example with `ansi-to-tui`).
- Parser: keep `pulldown-cmark` (with source offsets for the "shown as
  written" rule) or switch to `markdown` like patine.
- Which elements make sense in a question: code blocks and tables need
  width; option descriptions are narrow, and the image beside the
  question takes two fifths of the width.
- Code blocks are never wrapped in patine; in asqr's narrow areas they
  need horizontal cropping or scrolling.
- Version: 1.0.0 is not released; decide whether this goes in before or
  after. It needs an ADR, the spec, and the skill (which tells agents
  what they may use).
