---
title: "Show text with ANSI colours as a context entry"
kind: feature
component: tui
origin: discussion
tags: [ux]
---
# Show text with ANSI colours as a context entry

A context entry has one of four types: `image`, `code`, `markdown` and
`diff` (`ContextEntry` in `src/format/context.rs`). None of them shows
text in colours the asker chooses: a `code` entry is highlighted by
asqr itself, by its `language` or file extension. On 2026-10-22,
mockups of asqr's question tab had to be shown as PNG images, drawn by
a script on a grid of cells in colours sampled from a screenshot of the
person's terminal. Such images match only that theme, and asqr scales
them down to the pane, so their text is small until `z` shows them full
screen. A coloured command output, such as `git diff --color` or a test
run, has no way into the pane with its colours at all.

## Proposal

A context entry of a new type, with `content` or `path` like `code`,
whose text carries SGR escape sequences (colours, bold, dim, reversed,
underlined). asqr draws it cell by cell: named colours as the
terminal's own, so they follow the person's theme. Every other escape
sequence, such as cursor movement, is dropped, so an entry cannot change
the screen outside its text. Lines do not wrap and scroll sideways, as
in a `code` entry.

The new type belongs to format version 2 (ADR 51). An older asqr
refuses a session with a context entry of a type it does not know, with
an error that names the type as not supported by its version, lists the
types it supports and says that a newer asqr may support it
(`refused_entries` in `src/format/session.rs`). The new type goes into
`CONTEXT_ENTRY_TYPES` in `src/format/context.rs` too, which that error
lists; `the_entry_types_are_those_the_parser_knows` checks the list
against the variants.

## Open questions

- Which SGR attributes count, and whether a dropped sequence leaves a
  mark.

## Decisions

- 2026-10-22, user: the new type goes into format version 2, and an
  older asqr refuses it with a message that a newer asqr may support it.
- 2026-10-22, user: the type is named `ansi`.
- 2026-10-22, user: under `NO_COLOR` an `ansi` entry loses its RGB
  colours too. `strip_colours` in `src/tui/render/theme.rs` keeps RGB
  colours for images, so the entry needs a way of its own to drop them.
- 2026-10-22, user: no gutter with line numbers.
- 2026-10-22, user: the 1 MiB warning for `code`, `markdown` and `diff`
  entries applies, counted with the escape sequences.
