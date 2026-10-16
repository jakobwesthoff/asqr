# 34. Show code and Markdown entries through patine

Date: 2026-10-14

## Status

Accepted

Amends [28. Session and result format version 2 with a context field](0028-session-and-result-format-version-2-with-a-context-field.md)

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [49. Present problems through one component in three forms](0049-present-problems-through-one-component-in-three-forms.md)

## Context

The amendment of ADR 28 replaced the `text` entry by two types: `code`,
shown exactly as written and highlighted, and `markdown`, rendered. Until
now the context pane showed both as a placeholder (ADR 31). Step 4 of the
plan todo "Split the TUI into a question pane and a context pane" gives
each type a viewer behind the pane's `Viewer` trait.

patine, the user's Markdown renderer for the terminal, renders Markdown
and highlights code into style-neutral lines of styled spans since
version 1.5.0, published 2026-10-14, so that asqr can draw them. The todo
"Full Markdown in question texts, rendered like patine" uses the same
library for question texts.

## Decision

Taken with the user (plan, *Viewers*, and the step's todo):

- **patine:** highlighting and Markdown both come from patine 1.5's
  library: `highlight` for code and `render_lines` for Markdown. Both use
  syntect with two-face's grammars and the 8-colour ANSI theme, so the
  terminal's theme decides the colours. asqr depends on patine from
  crates.io without default features. Its `cli` feature only builds the
  `patine` binary, with clap and crossterm.
- **Code:** highlighted once, in `load`. The language is `language`,
  else the one the extension of `path`, then of `title`, names; without
  any of them the text stays plain. A dim gutter holds each line's
  number. Lines go through `SourceLine` (ADR 33): tabs to stops of 4,
  control characters as their pictures, widths in columns, a dim `›` at
  a cut and a dim `‹` where scrolling hides text. Lines do not wrap. The viewer reports the width of its widest
  line, so the pane scrolls sideways, and the gutter stays in place
  while it does.
- **Markdown:** rendered by patine for the body's width in `lay_out`,
  so only when the width changes. Wrapped, without a gutter and without
  sideways scrolling.
- **The adapter:** one module, `src/tui/render/patine_adapter.rs`,
  turns patine's lines into what asqr draws, for the context pane and
  for question texts alike. ratatui has one underline only, so a double
  underline, which patine gives level-one headings, becomes single;
  patine makes those headings italic too, which keeps them apart from
  level-two headings. A colour becomes `Color::Indexed(color.index())`,
  so it stays a slot of the terminal's palette and index 7 stays
  patine's white.
- **Unreadable files** show through the pane's placeholder, as for a
  diff: "code not shown" or "markdown not shown", the path and the
  reason.

Settled with the user in the review of this step's plan:

- **Markdown never cuts a line.** patine leaves some lines wider than
  the width it got: lines of code blocks and frontmatter, a word or URL
  longer than the width, a table that cannot shrink any further. asqr
  wraps such a line at the body's width. Each further row repeats the
  line's indent, the blanks and blockquote bars it starts with, so a
  code block's continuation stays inside the block, and then shows a dim
  `↪` before the rest. A list item's marker counts as indent too, as
  blanks: a `• ` after any blanks and bars, and a number such as `1. `
  or `10. ` only right after the bars, since a code block's line starts
  with blanks and may itself begin with `1. `. A nested numbered item,
  which patine indents with blanks, continues under its number. Nothing is lost, and the pane never scrolls
  sideways for Markdown. Sideways scrolling would move the wrapped prose
  out of view as well whenever one line of code is wide, and cutting
  would lose content.
- **One rule for Markdown everywhere:** the user decided the same for
  code blocks in question texts, so every place asqr renders Markdown
  wraps this way, through the adapter's `markdown_rows`.
- **Wide characters at the break** keep ADR 33's rule for an edge: a
  character that would cross the end of a row starts the next one, and
  the cell it leaves shows a blank in its style.

Agent's choices within that design:

- **A file name without an extension counts whole**, so `Makefile` and
  `Dockerfile` get their grammars: syntect's grammars list such names
  among their extensions. This applies to `title` too, so a title that
  happens to name a grammar, such as `Log`, picks it; `language` always
  takes precedence.
- **The first name that exists decides**, even one that patine does not
  know: `/tmp/out.txt` stays plain although its `title` might name a
  language. A wrong guess from the next field would be harder to explain
  than plain text.
- **The marker is `↪` and a blank.** The arrow points from the end of
  one row into the next, and its East Asian width is neutral, so it
  takes one column in every terminal. Where
  the indent leaves no room for text after it, further rows start with
  the marker alone, and in fewer than 3 columns the line breaks without
  one.
- **Empty text** shows a dim "empty" in place of lines.
- **Reading files** follows the diff viewer's rules (ADR 33), now in
  `src/tui/render/source_file.rs` for all three viewers: a relative path
  is never resolved, only a regular file is read and only its first
  8 MiB, a file with a NUL byte is binary and not shown, and bytes that
  are not UTF-8 show as `�`. Image entries refuse a path that is not a
  regular file the same way.
- **Rendering again for each new width:** the pane's keeper calls
  `lay_out` only when the size changes, which happens on a resize or on
  `z`. The Markdown viewer keeps the width it rendered for and returns
  the size it found then when only the height changed, as the rows
  depend on the width alone. Code and diffs render nothing in
  `lay_out`.
- **A new ADR** rather than an amendment of ADR 31 or 33: this decision
  brings a dependency and the adapter that neither covers, and question
  texts will use the adapter too.
- **Regained focus** (ADR 24) keeps both viewers with what they derived,
  as ADR 31 decided for every viewer that sends nothing to the terminal.

## Consequences

- The person sees code highlighted and Markdown rendered in the pane,
  where they saw a placeholder before.
- asqr depends on patine 1.5, and through it on `markdown`, `syntect`
  (with its pure-Rust regex engine), `two-face` and `anyhow`. The
  lockfile gains their dependencies.
- A wide line of code scrolls sideways in a code entry and wraps in a
  Markdown entry. The two types differ in this on purpose: a code entry
  keeps lines as written, Markdown fits the pane.
- `SourceLine` can now also break a line into rows, which the adapter
  uses and the question texts' Markdown will use as well.

## Amendment (2026-10-16, task boxes)

patine 1.7.0 draws a task list item's box in place of its bullet, `☐`
for an open task and `☑` for a done one, and the box of an ordered task
after its number, as `1. ☐ `. The continuation rows of a line too wide
for its space count these as a list item's marker too: `☐ ` and `☑ `
after any blanks and bars, like `• `, and a box right after a number
that counts, which widens the number's marker by two columns.
