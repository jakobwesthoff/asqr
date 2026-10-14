# 31. Make the context area a pane with a header, focus and scrolling

Date: 2026-10-14

## Status

Accepted

Amends [28. Session and result format version 2 with a context field](0028-session-and-result-format-version-2-with-a-context-field.md)

Amends [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md)

## Context

ADR 30 placed the context area beside or below the question. It drew an
image fit into the area and a placeholder for text and diff entries, and
`z` and `o` acted on images only (ADR 28). The plan todo "Split the TUI
into a question pane and a context pane" makes the area a pane of its
own in its third step: a header, viewers for each kind of entry, and
scrolling with its own keys. The text and diff viewers follow in the
plan's next two steps, and phase 2 shows several entries per question
with a tab strip.

## Decision

Taken with the user (plan, *Keys*):

- **Header:** the pane starts with a line that names the entry by its
  `title`, or by its file name.
- **Viewers:** each kind of entry is shown by a viewer, which lays its
  content out for the pane's width and height, says whether its lines
  scroll sideways, and draws itself at a scroll offset. Images are the
  first viewer; text and diff entries keep their placeholder, as a
  viewer, until their own viewers land.
- **`z`:** shows any entry full screen, not only an image. `o` still
  opens images only.

Agent's choices within that design:

- An entry without a title and without a path, such as inline text, is
  named by its kind (`text`, `diff`) in the header.
- When the content is taller than the pane, the header's right end says
  which lines show, such as `4–15 of 120`, as the intro overlay does
  (ADR 30).
- A row without context leaves the pane empty, header included, so the
  layout does not jump.
- The full-screen view is the pane over the whole tab area, header
  included, inside the same one-column margins as the tab.
- The hint that stands in for the pane on a small terminal offers `z`
  for every entry.
- Viewers live as long as their entry is shown in a waiting session,
  not one draw (settled in the review of the viewer interface). The text
  viewer reads a file, highlights it and renders Markdown; building it
  on every frame would redo that work each time. A viewer is rebuilt
  when a replaced session file puts another entry in its place, and
  dropped when its session leaves the queue.

## Consequences

- The text and diff viewers plug into the pane without changing it, and
  so does phase 2's tab strip above the body.
- The pane takes one row for its header, so an image beside or below
  the question gets one row less than before.
- `z` on a text or diff entry shows its placeholder full screen until
  the viewers land.
