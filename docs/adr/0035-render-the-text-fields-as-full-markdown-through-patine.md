# 35. Render the text fields as full Markdown through patine

Date: 2026-10-16

## Status

Accepted

Amends [9. Crate choices](0009-crate-choices.md)

Amended by [47. Put the session's title in the title bar](0047-put-the-sessions-title-in-the-title-bar.md)

Amended by [55. Wrap option labels and render them as Markdown](0055-wrap-option-labels-and-render-them-as-markdown.md)

Amended by [57. Show a Markdown table that does not fit as records](0057-show-a-markdown-table-that-does-not-fit-as-records.md)

Amended by [99. Show text fields the Markdown parser panics on as written](0099-show-text-fields-the-markdown-parser-panics-on-as-written.md)

## Context

The session's text fields are the intro, question texts and option
descriptions. In long, structured questions, inline code shown reversed
read as solid blocks, and bold labels did not stand out enough to
structure a text. The fields render full Markdown in the style of
patine. ADR 34 brought patine and the adapter `patine_adapter.rs` for
the context pane's Markdown entries.

## Decision

- **Full Markdown everywhere:** the intro, question texts and option
  descriptions follow one rule and render full Markdown.
- **patine:** the fields render through patine and the adapter of
  ADR 34, as Markdown entries do.
- **Nothing is cut:** a line patine leaves wider than its area, such as
  a line of a code block or frontmatter, a word longer than the width or
  a table that cannot shrink, wraps behind a dim `↪` through the
  adapter's `markdown_rows` (ADR 34). Nothing scrolls sideways.
- **The width in use:** a text renders for the width it gets in the
  frame being drawn.
- **Two rules of the format:** the fields keep two rules that askers
  write against, which the schema descriptions and the skill state
  (ADR 28). A single newline breaks the line, where CommonMark joins
  such lines into one paragraph. Raw HTML shows as written, so
  `Vec<String>` stays whole and an HTML block shows its lines; autolinks
  stay links.
- **In patine, not in asqr:** both rules are options of patine,
  `SoftBreaks::Break` and `Html::Literal`, which the fields pass to
  `render_lines_with`. asqr does not rewrite the Markdown before patine
  renders it.
- **Markdown entries keep CommonMark:** they are documents, so the
  context pane renders them with patine's defaults, which join single
  newlines and leave raw HTML out.
- **Quotes:** patine draws a quote's text in the style of the text
  around it and its bar in a colour chosen through
  `RenderOptions::with_quote_style`. asqr draws the bar blue, and dim
  without a colour under `NO_COLOR`: asqr takes the colours off the
  finished screen and keeps dim (ADR 15), so a blue bar would be left in
  the text colour, like the quoted text beside it. The text fields pass
  the quote style with their two options, which are built with it on
  each call. Markdown entries render through `render_lines_with` with
  the same quote style and patine's defaults otherwise. The screen picks
  the quote style for each frame from `NO_COLOR` and hands it to every
  place that renders Markdown. A context viewer built with another quote
  style is built again.
- **The intro** in the header shows patine's styles in the text colour
  and is cut as ADR 47 decides; the `i` frame scrolls over all its rows.
- **Option descriptions** render for the columns from the description
  column to the end of the line, beside the label or, where that leaves
  less than 30 columns, below it from column 6. Every row starts at that
  column, so a description keeps its hanging indent and patine's list
  indents and `↪` rows sit inside it.
- **The review tab** shows question headers, option labels and typed
  answers, none of the three fields, so it renders no Markdown. Headers
  and labels stay plain text.
- **The module** `src/tui/render/text.rs` holds the one call to patine
  for the three fields.
- **No render cache:** the screen's `View` keeps no rendered rows of the
  texts from one frame to the next. A text renders through a plain
  function from the text and a width to its rows. asqr draws once per
  event: the loop in `src/tui/run.rs` blocks until a key, a resize, a
  change in the inbox, regained focus or a finished load arrives, and
  has no tick. Large content belongs in the context pane, whose viewers
  keep their own layout per size (ADR 31).

## Consequences

- The person sees headings, quotes, links, tables, rules and highlighted
  code blocks rendered in the three fields. Inline code shows in yellow.
- A wrapped list item lines up past its bullet, as patine lays it out,
  and so does a line after a single newline inside a list item.
- Pieces glued together without a blank, such as `Vec` and `<String>`
  or `**bold**` and a comma after it, wrap as one word.
- Every frame renders the three fields again. On a 120×40 screen with a
  list, a Rust code block and four described options, a frame took about
  244 µs through patine in a release build and 3.1 ms in a debug build,
  far below what a person notices. patine's first rendering takes 34 ms
  once, for syntect's grammars.
- A list item wraps at the full width of its field, and a link whose
  text is its URL, such as an autolink, shows the URL once, also when
  dropped raw HTML wraps the URL. A block that opens a list item follows
  its bullet without a blank line.
- Quoted text keeps the style of the text around it, task items show
  `☐` or `☑`, and table columns follow their delimiter row.
