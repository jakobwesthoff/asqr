# 98. Show a Markdown entry as plain text when its parser panics

Date: 2026-10-31

## Status

Accepted

Amends [94. Show a Markdown entry above 1 MiB as plain text](0094-show-a-markdown-entry-above-1-mib-as-plain-text.md):
a second reason for a Markdown text to show as plain text

Amends [46. Build and lay out context viewers on worker threads](0046-build-and-lay-out-context-viewers-on-worker-threads.md):
a panic of the Markdown parser no longer ends asqr; the panic of any
other job still does

Amended by [99. Show text fields the Markdown parser panics on as written](0099-show-text-fields-the-markdown-parser-panics-on-as-written.md)

## Context

- The Markdown parser, `markdown` 1.0.0 behind patine, panics on short
  texts. Run through `patine::render_lines_with`, each of these ends the
  viewer:

  | Text | Panic |
  |---|---|
  | `[a]:b`, `=`, `=+` and `-` on four lines, the third ending in a carriage return | `Cannot push to non-parent` |
  | `1. ~~~` and `*` on two lines | `mismatched (non-jsx): ListOrdered / ListUnordered` |
  | `---`, a blank line and `- a`, or `-` | `index out of bounds` |
  | `---`, a blank line and `[^a]:` | `called Option::unwrap() on a None value` |

  The first two panic with the default and the GFM options too. The last
  two panic with the frontmatter construct on, which patine turns on.
- A job's panic is handed to the drawing thread, which panics with its
  message (ADR 46), so a Markdown entry the parser panics on ends asqr.
- The size limit of ADR 94 already makes a Markdown text show as plain
  text, wrapped, below a dim note.

## Decision

- **A panic of the Markdown parser while a Markdown viewer lays out its
  text makes the viewer show the text as plain text,** as above 1 MiB,
  below the dim note "Markdown failed: shown as plain text". The note is
  36 columns, so it shows whole in the narrowest body of the context
  pane, 38 columns.
- **The decision is kept for the viewer,** so another width does not ask
  the parser again.
- **The panic is contained where the parser runs,** in the Markdown
  viewer, so a caption of an image entry, which is rendered as a Markdown
  entry is (ADR 69), shows as plain text the same way. Jobs for other
  kinds of entry keep the handling of ADR 46; they do not run the
  Markdown parser.
- **The contained panic does not reach the terminal.** The panic hook of
  the binary gives the terminal back unless the thread runs a job, so the
  call marks the thread as running one for its duration.
- **The log:** one `warn` line, "the Markdown parser panicked; showing
  the text as plain text", with the panic message, which names a place in
  the parser's code and not the text.
- **The note comes first,** above the notes of a cut (ADR 75), as the
  note on a text above 1 MiB does.
