---
title: "Show text with ANSI colours as a context entry"
kind: feature
component: tui
status: blocked
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

The new type belongs to format version 2. An older asqr refuses a
session with a context entry of a type it does not know
(`an_unknown_type_is_no_context_entry` in `src/format/context.rs`), so
its message has to say first that a newer asqr may support the type.

## Open questions

- `NO_COLOR`: `strip_colours` in `src/tui/render/theme.rs` resets named
  and indexed colours but keeps RGB ones, which images need. RGB
  colours of the new entry would then survive `NO_COLOR`.
- The type's name, such as `ansi` or `terminal`.
- Which SGR attributes count, and whether a dropped sequence leaves a
  mark.
- Whether the entry has a gutter with line numbers, as a `code` entry
  does.
- Whether the 1 MiB warning for `code`, `markdown` and `diff` entries
  applies, counted with or without the escape sequences.

## Decisions

- 2026-10-22, user: the new type goes into format version 2, and an
  older asqr refuses it with a message that a newer asqr may support it.

## Relations

- Depends on: [Explain that an unknown context type may need a newer asqr](01M5HKZWDPH20QX3841GES7K8P-explain-that-an-unknown-context-type-may-need-a-newer-asqr.md)
