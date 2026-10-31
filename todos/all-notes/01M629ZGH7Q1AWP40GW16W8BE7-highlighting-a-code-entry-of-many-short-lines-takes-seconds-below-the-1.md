---
title: "Highlighting a code entry of many short lines takes seconds below the 1 MiB limit"
kind: improvement
component: tui
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

## Decisions (Jakob, 2026-10-30, in the chat)

- **A line limit beside the 1 MiB limit.** A code entry or diff side is
  highlighted for its first 50,000 lines; the lines after them show
  plain at once. It works with patine's `highlight` as it is, which
  takes a whole text and cannot be stopped (ADR 0046). Rejected: plain
  first with colours arriving in parts, which needs a partial-highlight
  API in patine and reverses ADR 0046's "no plain first pass"; a time
  budget, which needs the same API.
- **N = 50,000 lines,** about 0.45 s for Rust and about 2 s for a log
  at worst (patine 1.8 figures). Ordinary code below 1 MiB holds 22,000
  to 32,000 lines and stays fully coloured; only generated text with
  very short lines is cut. Rejected: 20,000; 100,000.
- **A dim note at the cut,** such as "highlighted for 50,000 lines; the
  rest is plain", in the style of ADR 0075's cut notes (lead's
  recommendation, shown in the mockup Jakob chose).
- **Covers code entries and diff sides,** which share `HIGHLIGHT_LIMIT`
  today (`src/tui/render/code.rs:71`, `src/tui/render/diff.rs:295`).
  Fenced code blocks in Markdown stay as they are; Markdown above 1 MiB
  shows as plain text (ADR 94). Rejected:
  Markdown code blocks too; code entries only.

## Technical notes (lead, 2026-10-30)

- Re-measure on patine 1.9.0 before N is fixed, in a release build of a
  scratch crate outside the repository: 500,000 lines of `x` as Rust, a
  1 MiB Rust sample and a 1 MiB log, each under `/usr/bin/time -l`. The
  `HIGHLIGHT_LIMIT` doc comment still derives 1 MiB from patine 1.7.1;
  patine 1.8.0 made log highlighting about five times faster
  (patine CHANGELOG), so the comment is rewritten with the new numbers.
- Highlight a prefix of the text; `diff.rs` already has a `first_lines`
  helper. Line ends follow ADR 0075 (`\n`, `\r\n`, a lone `\r`).
- Tests at N and N + 1 lines; no timing test.
- Docs: spec on code and diffs (around `docs/spec.md:2489-2493` and
  2550-2556), a new ADR amending ADR 0046 only for the limit (its "no
  plain first pass" stays for the highlighted part).
