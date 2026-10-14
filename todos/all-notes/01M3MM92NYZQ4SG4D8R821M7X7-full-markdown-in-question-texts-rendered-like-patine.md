---
title: "Full Markdown in question texts, rendered like patine"
kind: feature
component: tui
horizon: next
origin: request
tags: [ux, milestone/v0-11]
---
# Full Markdown in question texts, rendered like patine

The user requested this on 2026-09-28 and again on 2026-10-14. Question
texts, the intro and option descriptions render full Markdown with
syntax highlighting, in the restrained style of the user's tool patine
(`~/Development/github/jakobwesthoff/patine`, version 1.5.0, tag
`v1.5.0`, on crates.io). It ships in 0.11.0 together with the context
pane, whose Markdown viewer uses the same renderer.

## The request

On 2026-10-14 the user raised it again after answering long, structured
questions through asqr: questions that explain a proposal step by step,
with lists, inline code and emphasis, are harder to read than they
should be. The rendering should follow patine's design: a little colour
and clear but restrained highlighting.

Example: the `auto-rule` question of asqr session
`01M4XFKQ2V7NK2NWKAP9QEEZB2` (round 2 of the split layout discussion)
packs a rule with four bullet points, inline code, bold labels and a
list of examples into one question text. Inline code renders reversed
(`markdown.rs`, `Event::Code`), which makes every code span a solid
block, and the bold labels do not stand out from the surrounding text
enough to structure it.

## Where asqr stands

Text fields (`intro`, question `text`, option `description`) support a
small subset: bold, italic, inline code, lists and line breaks. Every
other element is shown exactly as written in the source (spec sections 4
and 7.6, `src/tui/render/markdown.rs`, parsed with `pulldown-cmark`,
used only from `src/tui/render/screen.rs`).

## How patine renders (from its README and source)

- Less color, more structure: the terminal's default colors, bold,
  italic, underline and dim. Where it colors, only the 8-color ANSI
  palette, so the user's terminal theme applies.
- Headings: H1 bold, italic and double-underlined; H2 bold and
  underlined; H3 to H6 bold (`src/render.rs`, heading styles).
- Inline: bold, italic, bold-italic, strikethrough, inline code in the
  palette's dark yellow (`CODE_COLOR`, `Color::DarkYellow`), not
  reversed.
- Lists with `• ` bullets, wrapped lines aligned past the bullet.
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

## Proposal

Decided by the user on 2026-10-14 (asqr session
`01M4XHEY66BQDA0ZPRXV3P22MD`): patine's library renders to
style-neutral lines of styled spans, and asqr draws those lines. The
patine side, with its design and size, is patine todo
01M4XHNXXNJNV57SNBYH88EFAN; it shipped as patine 1.5.0 on 2026-10-14.

On asqr's side:

- A small adapter from patine's `TextStyle` to ratatui's `Style`
  (double underline becomes single, as ratatui has no double
  underline). The context pane's Markdown viewer uses it too; whichever
  lands first builds it.
- It replaces `src/tui/render/markdown.rs` and `pulldown-cmark` for the
  intro, question texts and option descriptions. asqr renders again at
  the new width on resize and caches per width.
- Full Markdown everywhere: the intro, question texts and option
  descriptions follow one rule (user, 2026-10-14). patine wraps tables
  to the width it gets; the skill advises keeping descriptions short.
- A code block line longer than the question pane wraps and is marked
  as a continuation, so nothing is lost (user, 2026-10-14). patine
  itself never wraps code, so asqr wraps its lines after rendering.
- The spec (section 7.6's subset rule), the skill (which tells agents
  what they may use, and that long code belongs in a code entry of the
  context pane) and an ADR.
- asqr depends on patine 1.5.0 from crates.io (`cargo add patine@1.5`,
  published 2026-10-14), without a path or git dependency (user, asqr
  session `01M4XJX6BJADVKKXNXXFNVVC8Q`).
