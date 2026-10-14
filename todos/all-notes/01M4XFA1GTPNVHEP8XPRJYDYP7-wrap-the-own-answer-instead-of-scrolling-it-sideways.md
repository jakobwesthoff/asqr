---
title: "Wrap the own answer instead of scrolling it sideways"
kind: improvement
component: tui
horizon: next
origin: request
tags: [ux, milestone/v0-11]
---
# Wrap the own answer instead of scrolling it sideways

The user asked for this on 2026-10-14. A long own answer (the `custom`
row of a `single` or `multi` question) stays in one screen line and
scrolls sideways while typed, and is cut off with `…` otherwise. A long
note wraps onto further lines, aligned under its start, which is much
easier to read. The own answer should wrap the same way.

## Where asqr stands

- `src/tui/render/screen.rs`, `one_line_field`: renders the own-answer
  row and, through the same function, the reject reason on the review
  tab. While focused it shows a window of the text that ends just after
  the text cursor, with `…` for text scrolled out on the left; unfocused
  it cuts the text at the line's end with `…`. The counter sits at the
  end of the line.
- `text_field` in the same file renders a `text` question's answer
  and wraps each line with `wrap`, indented under the first line, with
  the counter on a line of its own after the last one. The note renders
  the same way.
- Spec section 7.3 and ADR 0022 say the own answer "is one line and
  scrolls sideways", and the reject reason is "a one-line field like the
  own answer". The snapshot test
  `the_own_answer_is_typed_in_its_line_and_scrolls_sideways` pins the
  current behaviour.

## Proposal

- Wrap the own answer's text across as many screen lines as it needs,
  indented under the start of the text after the row's head, focused or
  not. Nothing is cut off with `…` any more.
- The value stays one line: `ctrl-j` adds no line break, and the format
  keeps own answers single-line (ADR 0007).
- `↑`/`↓` keep leaving the row instead of moving between wrapped
  lines, as spec 7.3 describes.
- The counter moves after the last wrapped line, as in `text_field`.
- The reject reason shares `one_line_field` and wraps the same way, so
  both one-line fields behave alike (user, 2026-10-14).
- Update spec section 7.3 with an ADR that amends ADR 0022, and replace
  the sideways snapshot with one that shows the wrap.

## Done when

- A long own answer, typed or not, shows completely on wrapped lines in
  every terminal width from 60 columns on.
- The text cursor shows on the right wrapped line while typing.
- The row under the cursor keeps the current question in view
  (`a_small_screen_keeps_the_cursor_row_in_view`), with a wrapped own
  answer counting all its lines.

## Relations

- Relates to: [Split the TUI into a question pane and a context pane](01M4XEGTAZ69KV9Y0ZTNYQJJ4X-split-the-tui-into-a-question-pane-and-a-context-pane.md), the question pane gets a reading width there, which makes a sideways-scrolling own answer shorter still
