# 99. Show text fields the Markdown parser panics on as written

Date: 2026-10-31

## Status

Accepted

Amends [98. Show a Markdown entry as plain text when its parser panics](0098-show-a-markdown-entry-as-plain-text-when-its-parser-panics.md):
the fields render on the drawing thread, and show the text as written
without a note

Amends [35. Render the text fields as full Markdown through patine](0035-render-the-text-fields-as-full-markdown-through-patine.md):
a text the parser panics on shows as written

Amends [55. Wrap option labels and render them as Markdown](0055-wrap-option-labels-and-render-them-as-markdown.md):
a label the parser panics on shows as written

## Context

- The intro, the question texts, the option labels and descriptions, the
  intro overlay and the labels in the review render through
  `patine::render_lines_with` on the drawing thread (`src/tui/render/text.rs`),
  not in a loading job. The texts that make the parser panic are those of
  ADR 98, `1. ~~~` and `*` on two lines among them.
- A panic on the drawing thread reaches the panic hook of the binary,
  which gives the terminal back and ends asqr.
- The fields render every frame, once to count their rows and once to
  draw them.

## Decision

- **A text the parser panics on shows as written:** its lines unstyled,
  wrapped as any field line, with the first line of an option's label
  leaving room for the marker of a default option (ADR 79). Rows
  counted equal rows drawn.
- **No note.** A question line, a label or a line of the review has no
  place for one.
- **A text that made the parser panic is remembered** by a fingerprint
  of the text, on the drawing thread, up to 64 texts, after which the
  memory starts over. The parser is not asked about it again, and the log
  gets one `warn` line, "the Markdown parser panicked; showing the text
  as plain text", with the panic message, which names a place in the
  parser's code and not the text.
- **The panic is contained as ADR 98 describes.**
