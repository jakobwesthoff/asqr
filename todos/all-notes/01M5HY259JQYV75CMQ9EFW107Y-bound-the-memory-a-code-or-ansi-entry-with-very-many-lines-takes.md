---
title: "Bound the memory a code or ansi entry with very many lines takes"
kind: improvement
component: tui
status: needs-discussion
origin: implementation
tags: [ux]
---
# Bound the memory a code or ansi entry with very many lines takes

Measured on 2026-10-22 while building the `ansi` entry (release build,
peak memory of a probe that builds the viewer's content): 8 MiB of
nothing but line breaks, about 8.4 million empty lines, takes 828 MB in
the code viewer and 551 MB in the `ansi` viewer, about 100 bytes per
empty line in the code viewer. asqr reads at most the first 8 MiB of a
file (`MAX_BYTES` in `src/tui/render/source_file.rs`), so this is the
worst case per entry, and asqr keeps the viewers of three questions at a
time (ADR 46), so a few such entries in a row can take several GB.
Ordinary files are far from it: a 1 MiB log of 20,000 lines costs about
2 MB this way.

## Why

The code viewer (`src/tui/render/code.rs`) builds every line once as a
row of its own: its number and a `SourceLine` with a list of pieces,
each piece with its own text and style. An empty line still carries
all of that. Whether the Markdown viewer behaves alike is not measured.

## Options

Not decided:

- **Limit the lines as well as the bytes.** An entry of millions of
  empty lines makes no sense and is more likely an error or misuse than
  material to read. Beside the 8 MiB limit, a maximum number of lines
  cuts the entry off with a note, as the byte limit does today. The
  limit would be set from what a realistic file of this size has in
  lines.
- **Keep lines leaner.** Store empty lines and lines in a single style
  without a list of their own, or build lines only when they are drawn.

The two can be combined.

## Open questions

- Which line limit fits 8 MiB of realistic content, and whether it also
  applies to `markdown` and `diff` entries.
- Whether `validate` and `ask` warn about an entry above the line limit,
  as they warn above 1 MiB.

## Decisions

- 2026-10-22, user: worth a todo of normal impact; limiting the line
  count in addition to the bytes is the user's own idea, as such input
  looks like an error or misuse rather than something to optimise for.
