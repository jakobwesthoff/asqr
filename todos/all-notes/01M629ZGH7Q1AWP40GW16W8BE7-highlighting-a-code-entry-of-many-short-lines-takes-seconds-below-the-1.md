---
title: "Highlighting a code entry of many short lines takes seconds below the 1 MiB limit"
kind: improvement
component: tui
status: needs-discussion
origin: review
---
# Highlighting a code entry of many short lines takes seconds below the 1 MiB limit

Found in a plan review on 2026-10-28. A code entry under 1 MiB is always
highlighted, and the time that takes follows its lines: 500,000 lines of
`x` take about 4.5 s. The 500,000-line limit of ADR 0075 bounds the
memory, not this time.

## Context

- `HIGHLIGHT_LIMIT` in `src/tui/render/code.rs` is 1 MiB. `language_for`
  highlights a text up to that many bytes and leaves a longer one plain.
  The diff viewer uses the same limit (`src/tui/render/diff.rs`).
- The limit's doc comment derives it from logs measured with patine
  1.7.1, about 54 µs a line, the smaller of them 1,045,937 bytes and
  14,154 lines, and says a log under 1 MiB is highlighted in under a
  second.
- ADR 0075, Consequences: 500,000 lines of `x` in Rust, under 1 MiB,
  took 4.47 s to highlight.
- Reproduced on 2026-10-28: patine 1.8.0's `highlight` over 500,000
  lines of `x`, 1,000,000 bytes, as Rust took 4.51 s and a peak of
  98.4 MB, in a release build under `/usr/bin/time -l` on macOS. That is
  about 9 µs a line, over 35 times as many lines as the log.
- The code viewer shows its entry only once it is highlighted (doc
  comment of `HIGHLIGHT_LIMIT`). Loading runs on a thread of its own, so
  keys work meanwhile (ADR 0046).

## Options

- A line limit for highlighting, beside the byte limit.
- Show the text plain at once and highlight it in parts, the part in
  view first.
- A time budget, after which the rest stays plain.

## Open questions

- Which of these.
- For a line limit, which count, and measured with which grammars: the
  doc comment notes that other grammars can be slow too.
