---
title: "Gather the TUI colours into colour roles and honour NO_COLOR"
kind: refactor
component: tui
origin: discussion
tags: [ux]
---
# Gather the TUI colours into colour roles and honour NO_COLOR

Chosen by the user on 2026-10-15 in the review of how the TUI could look
better with the colour scheme of `docs/design-guide.md`. Today the colours
are written where they are used: `Color::` appears in `screen.rs`,
`text_field.rs`, `diff.rs`, `code.rs`, `patine_adapter.rs` and other files
under `src/tui/`. A colour scheme for the TUI needs one place that says
which role gets which colour, and asqr does not read `NO_COLOR` today.

## Goal

One module, such as `src/tui/render/theme.rs`, that names the roles the
TUI draws with: the primary colour, the highlight, focus, a primary button,
success, warning and error, plus dim. Every place that colours something
asks for a role instead of a `Color`. The design guide's terminal section
says which ANSI colour each role is: named and indexed colours only, so the
terminal theme decides the shades (ADR 33 to 35).

`NO_COLOR` (https://no-color.org) set and not empty removes the colours of
every role and keeps the modifiers, so meaning still shows through text,
marks, bold, dim and reversed, as ADR 8 requires.

## Open questions

- Which colour each role gets is the subject of the colour-role todo that
  follows the comparison of variants; this todo can land with today's
  colours behind the roles.
- Whether patine's colours for code and Markdown go through the roles or
  stay as they are (ADR 34).
