---
title: "Laying out a large Markdown entry takes seconds and hundreds of MB"
kind: improvement
component: tui
origin: review
---
# Laying out a large Markdown entry takes seconds and hundreds of MB

Found in a plan review on 2026-10-28. The 500,000-line limit of ADR 0075
bounds the memory of the code, ANSI and diff viewers, but not of the
Markdown viewer. patine's layout costs memory by the byte of Markdown,
and its time grows faster than the text.

## Context

ADR 0075, Consequences: 8 MiB of real Markdown, 173,962 lines, took
693.1 MB and 13.6 s to lay out, and the line limit does not change that.
That was a release build measured with `/usr/bin/time -l` on macOS.

A second measurement on 2026-10-28 called patine 1.8.0's
`render_lines_with` alone, 80 columns wide, in a release build under
`/usr/bin/time -l` on macOS. The input was Markdown files from local
repositories, joined and cut to each size; the 8 MiB text had 171,455
lines.

| Size | Time | Peak memory |
|---|---|---|
| 0.5 MiB | 0.16 s | 102.6 MB |
| 1 MiB | 0.56 s | 233.5 MB |
| 2 MiB | 1.82 s | 345.9 MB |
| 4 MiB | 5.46 s | 496.6 MB |
| 8 MiB | 14.90 s | 796.9 MB |

Each doubling of the size took about three times as long. Peak memory
per byte of Markdown ran from 95 bytes at 8 MiB to 223 bytes at 1 MiB;
ADR 0075's run comes to 83.

Where this applies:

- The Markdown viewer (`src/tui/render/markdown_viewer.rs`) lays out the
  whole text for the body's width, and again whenever the width changes.
- A file is read up to 8 MiB (`src/tui/render/source_file.rs:267`).
  Inline text has no byte limit; since ADR 0075 its lines are cut at
  500,000 like a file's (`source_file.rs:270-285`). Real Markdown never
  reaches that line count below 8 MiB, so the cut does not lower its
  cost.
- asqr keeps the viewers of three questions at a time (ADR 0046).
- Layout runs on a loading thread, and keys work meanwhile (ADR 0046).
- `asqr validate` and `asqr ask` warn about an entry above 1 MiB.

## Decisions (Jakob, 2026-10-30, in the chat)

- **Above 1 MiB a Markdown entry shows as plain text,** wrapped to the
  body, whole, with a dim note at the top that it is shown as plain
  text. Nothing is lost, and it is the rule the code viewer follows
  above `HIGHLIGHT_LIMIT` (`src/tui/render/code.rs:71`). Rejected:
  cutting the text at the limit; rendering only the part in view, which
  needs a range API in patine and breaks references, footnotes and list
  numbering across blocks (size L).
- **The limit is 1 MiB,** the number the warning of `asqr validate` and
  `asqr ask` and the highlighting limit already use. Rejected: 512 KiB,
  a second number.
- **Inline Markdown counts the same as a file.** The viewer costs the
  same for either source, and the warning already counts inline text.
  Rejected: keeping inline text whole.

## Technical notes (lead, 2026-10-30)

- The cost of wrapping plain text is not measured; the plan measures it
  for 1 to 8 MiB before the plain path is built.
- The warning text of `validate` and `ask` (`src/format/warnings.rs:89`)
  can say that such an entry shows as plain text.
- A blank-only cut text skips patine already (`markdown_viewer.rs:62-79`);
  with the byte limit, a text of almost only blank lines above 1 MiB
  takes the plain path too.
- Docs: spec on text limits (around `docs/spec.md:2427-2442`) and the
  warnings (around 298-315); a new ADR that amends ADR 0075 (its
  Context sentence on inline text) and ADR 0034.
- `HIGHLIGHT_LIMIT` is shared with the code highlighting todo
  01M629ZGH7Q1AWP40GW16W8BE7; both keep 1 MiB, so they can run apart.
