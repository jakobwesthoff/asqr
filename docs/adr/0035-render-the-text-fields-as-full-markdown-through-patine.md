# 35. Render the text fields as full Markdown through patine

Date: 2026-10-14

## Status

Accepted

Supersedes the Markdown subset of [7. Session and result format version 1](0007-session-and-result-format-version-1.md)

Amends [9. Crate choices](0009-crate-choices.md)

## Context

ADR 7 limited the text fields, the intro, question texts and option
descriptions, to a Markdown subset: bold, italic, inline code, lists and
line breaks. asqr rendered it with `pulldown-cmark` (ADR 9) and showed
every other element as written in its source.

The user asked on 2026-09-28 and again on 2026-10-14 for full Markdown
in these fields, rendered in the style of patine, after answering long,
structured questions through asqr: inline code showed reversed, as solid
blocks, and bold labels did not stand out enough to structure a text.
ADR 34 brought patine 1.5 and the adapter `patine_adapter.rs` for the
context pane's Markdown entries.

## Decision

Taken with the user (todo "Full Markdown in question texts, rendered
like patine"):

- **Full Markdown everywhere:** the intro, question texts and option
  descriptions follow one rule and render full Markdown.
- **patine:** the fields render through patine and the adapter of
  ADR 34, as Markdown entries do. `src/tui/render/markdown.rs` and
  `pulldown-cmark` go.
- **Nothing is cut:** a line patine leaves wider than its area, such as
  a line of a code block or frontmatter, a word longer than the width or
  a table that cannot shrink, wraps behind a dim `↪` through the
  adapter's `markdown_rows` (ADR 34). Nothing scrolls sideways.
- **The width in use:** a text renders for the width it gets in the
  frame being drawn.
- **Single newlines break the line:** the subset broke the line at a
  single newline, and the fields keep that. CommonMark joins such lines
  into one paragraph.
- **Raw HTML shows as written:** the subset showed it as written, and
  the fields keep that, so `Vec<String>` stays whole and an HTML block
  shows its lines. patine 1.5 left raw HTML out. Autolinks stay links.
- **In patine, not in asqr:** both rules are options of patine 1.6,
  `SoftBreaks::Break` and `Html::Literal`, which the fields pass to
  `render_lines_with`. asqr does not rewrite the Markdown before patine
  renders it.
- **Markdown entries keep CommonMark:** they are documents, so the
  context pane renders them with patine's defaults, which join single
  newlines and leave raw HTML out.

Agent's choices within that design, confirmed in the review of the
plan:

- **The intro stays dim** under the title: dim is added on top of
  patine's styles, which stay underneath. The `i` frame shows the intro
  without the dim, as it did before.
- **The cut intro** keeps the first three rendered rows, one on
  terminals under 24 rows, and the last ends in `… i: more`. The `i`
  frame scrolls over all rows.
- **Option descriptions** render for the columns from the description
  column to the end of the line, beside the label or, where that leaves
  less than 30 columns, below it from column 6, as before. Every row
  starts at that column, so a description keeps its hanging indent and
  patine's list indents and `↪` rows sit inside it.
- **The review tab** shows question headers, option labels and typed
  answers, none of the three fields, so it renders no Markdown. Headers
  and labels stay plain text.
- **The module** `src/tui/render/text.rs` holds the one call to patine
  for the three fields, with their options in a constant.

## Consequences

- The person sees headings, quotes, links, tables, rules and highlighted
  code blocks rendered in the three fields, where the subset showed
  their source. Inline code shows in yellow instead of reversed.
- A wrapped list item lines up past its bullet, as patine lays it out,
  and so does a line after a single newline inside a list item.
- Text and HTML glued together without a blank, such as `Vec` and
  `<String>`, are two elements to patine and can wrap apart at the end
  of a line.
- Every frame renders the three fields again. On a 120×40 screen with a
  list, a Rust code block and four described options, a frame took
  87 µs with the subset and about 244 µs through patine in a release
  build, 3.1 ms in a debug build. patine's first rendering takes 34 ms
  once, for syntect's grammars.
- asqr depends on patine 1.6 and no longer on `pulldown-cmark`.

## Amendment (2026-10-14, no render cache)

The first version of this decision kept the rendered rows of each text
per width in the screen's `View`, from one frame to the next. The user
decided the same day to remove that cache, so a text renders through a
plain function from the text and a width to its rows. The reasons:

- asqr draws once per event: the loop in `src/tui/run.rs` blocks until
  a key, a resize, a change in the inbox or regained focus arrives, and
  has no tick.
- An uncached frame of the screen above takes about 244 µs in a release
  build and 3.1 ms in a debug build, far below what a person notices.
- Large content belongs in the context pane, whose viewers keep their
  own layout per size (ADR 31).
