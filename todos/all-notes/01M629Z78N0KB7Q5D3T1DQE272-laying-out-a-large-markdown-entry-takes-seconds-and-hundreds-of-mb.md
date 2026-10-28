---
title: "Laying out a large Markdown entry takes seconds and hundreds of MB"
kind: improvement
component: tui
status: needs-discussion
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
- A file is read up to 8 MiB, and inline text is taken whole
  (ADR 0075, Context).
- asqr keeps the viewers of three questions at a time (ADR 0046).
- Layout runs on a loading thread, and keys work meanwhile (ADR 0046).
- `asqr validate` and `asqr ask` warn about an entry above 1 MiB.

## Options

- A byte limit for rendering Markdown, above which the text shows
  plain, as the code viewer stops highlighting above 1 MiB
  (`HIGHLIGHT_LIMIT` in `src/tui/render/code.rs`).
- Lay out only the part in view and what lies near it.
- Make the layout cheaper in patine (`../patine`).

## Open questions

- Which of these, or which combination.
- Where a byte limit would sit: 1 MiB already took 0.56 s and 233.5 MB
  in patine alone.
