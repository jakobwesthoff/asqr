---
title: "Colour the chosen mark of the own answer like the option marks"
kind: improvement
component: tui
origin: implementation
tags: [ux]
---
# Colour the chosen mark of the own answer like the option marks

Found while colouring the TUI by roles (ADR 37, 2026-10-15). An option
row draws its chosen mark, `(•)` or `[x]`, bold in the primary colour.
The own answer's chosen mark keeps the text colour, although the design
guide's terminal table puts chosen marks under the primary role without
exception.

## Problem

`question_content` in `src/tui/render/content.rs` builds the own
answer's head (pointer, mark, `✎` and label) as one `Span`, because
`FieldShape::head` in `src/tui/text_field.rs` is a single
`Span<'static>`. One span has one style, so the mark cannot take a
colour apart from the rest of the head. An option row has no field, so
its head is a list of spans and the mark is one of them.

## What a fix touches

- `FieldShape::head` becomes a list of spans or a `Line`, with every
  user of it in `text_field.rs`: `layout`, which turns the head into
  glyphs from its content and style and measures its width, and
  `resting_lines`, `counted_lines`, `TextField::lines` and the row and
  cursor computations that pass `&shape.head` to `layout`.
- `FieldRow` and `live_field` in `content.rs`, where the focus fill then
  goes onto each span of the head through `theme::focus_span`.
- The tests that build a head with `Span::from`.

Under the focus fill the mark takes the fill's colour like the rest of
its row (ADR 37), so the change shows only on a row without the fill.
