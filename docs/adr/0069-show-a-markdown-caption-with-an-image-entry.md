# 69. Show a Markdown caption with an image entry

Date: 2026-10-28

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
an image entry takes an optional `caption`

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md):
the full-screen view shows a captioned image alone, and from the
one-line hint first with its caption

Amended by [75. Cut a text entry after 500,000 lines](0075-cut-a-text-entry-after-500000-lines.md)

## Context

- An image entry carried a `title` and its image only (ADR 28, ADR 32).
  An agent presenting generated mascot drafts needed the prompt of each
  draft and its notes on it next to the image; the text went into
  separate questions, and the person never saw a draft next to its
  prompt.
- An image scales to fill the pane as far as its proportions allow
  (ADR 32), so a square image takes every row of the pane's body.
- ratatui-image draws an image partly scrolled off the pane for half
  blocks and the Kitty protocol only; for iTerm2 it needs one image per
  row, and for Sixel it cuts 6-pixel bands and needs a `Picker`, whose
  constructors change tmux's passthrough setting, which asqr manages
  itself (ADR 44).

## Decision

- An image entry takes an optional `caption`: Markdown, inline only,
  rendered like the content of a `markdown` entry. A prompt and a few
  notes fit inline, and inline means less format and fewer error cases.
  A caption that is empty or only whitespace is none.
- Only image entries take a caption: the need is shown only for images,
  and code and diff entries bring their own text. A caption on another
  entry is an unknown field.
- The caption is added within format version 2 as an optional field.
- **Fit:** the caption stands under the image, after a blank row, and
  the image is scaled to the rows the caption leaves, so image and
  caption always show together and nothing scrolls; `z` shows the image
  large. Fit behaves the same in every terminal, while an image drawn
  partly scrolled off would work only for half blocks and Kitty.
- Where the caption leaves the image fewer than 3 rows, the pane shows
  the caption alone, under a dim `z shows the image`, and the caption
  scrolls when it alone is taller than the pane, so no text is ever out
  of reach.
- The full-screen view (`z`) reached from a pane shows the image alone:
  it is for looking at the image closely, and the caption is read in the
  pane. Where only the one-line hint shows, the caption has no pane, so
  `z` shows image and caption by the same rule at the full-screen size,
  the next `z` the image alone and the one after that leaves; the key
  bar and the help say what the next `z` does.
- The full-screen view lays the same viewer out again, so it shows the
  image as it was read first.
- The skill tells agents to keep a caption to one short line, two at
  most, with only what matters about the image, and to put longer
  material, such as the full prompt or a list of notes, into the
  question's text or another context entry: a caption shrinks the image,
  and it is meant to say only the essentials.
- `asqr validate` and `asqr ask` warn about a caption above 1 MiB, as
  about any text entry; the image file itself still gets no such warning.

## Consequences

- A long caption makes the image small, and one that leaves it fewer
  than 3 rows hides it from the pane until the person presses `z`.
- An asqr that knows no `caption` ignores the field, as unknown fields
  are ignored when parsing, and shows the image alone.
