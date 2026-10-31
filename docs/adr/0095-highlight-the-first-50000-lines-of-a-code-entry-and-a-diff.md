# 95. Highlight the first 50,000 lines of a code entry and a diff

Date: 2026-10-31

## Status

Accepted

Amends [46. Build and lay out context viewers on worker threads](0046-build-and-lay-out-context-viewers-on-worker-threads.md):
highlighting stops after 50,000 lines as well as above 1 MiB; its "no
plain first pass" stays for the highlighted part

Amends [60. Highlight a diff's unchanged lines and fill its changed ones](0060-highlight-a-diffs-unchanged-lines-and-fill-its-changed-ones.md):
a diff's unchanged lines are highlighted for the first 50,000 lines of
the new text as well

## Context

- A code entry and a diff highlight up to 1 MiB (ADR 46, ADR 60). A code
  entry shows only once it is highlighted, with no plain first pass
  (ADR 46), on a loading thread so that keys keep working.
- patine's `highlight` takes a whole text and cannot be stopped.
- The time follows the lines of a text, not its bytes. ADR 75 measured
  500,000 lines of `x` in Rust, under 1 MiB, at 4.47 s.
- patine 1.9.0, release build, `/usr/bin/time -l` on macOS:

  | Text | Lines | Time |
  |---|---|---|
  | `x` lines as Rust | 500,000 | 4.92 s |
  | `x` lines as Rust | 50,000 | 0.50 s |
  | A log cut to short lines, 970,801 bytes | 50,000 | 0.56 s |
  | 1 MiB of Rust sources | 28,249 | 1.65 s |
  | 1 MiB of a log | 7,612 | 0.63 s |
  | `x` lines, no language | 500,000 | 0.023 s |

- The comment on the 1 MiB limit derived it from patine 1.7.1 and said a
  log under 1 MiB is highlighted in under a second.

## Decision

- **A line limit beside the 1 MiB limit:** a code entry or the new side
  of a diff is highlighted for its first 50,000 lines. The lines after
  them show plain at once. This works with patine's `highlight` as it is:
  the text is cut after line 50,000, the head is highlighted, and the
  rest goes through patine without a language. Rejected: showing the
  text plain first and sending colours in parts, which needs a
  partial-highlight API in patine and reverses ADR 46's "no plain first
  pass"; a time budget, which needs the same API.
- **50,000 lines:** about 0.5 s in the table above. Ordinary code below
  1 MiB holds 22,000 to 32,000 lines and stays fully coloured; only
  generated text with very short lines is cut. Rejected: 20,000 and
  100,000.
- **Covers code entries and diff sides,** which share the 1 MiB limit.
  Fenced code blocks in Markdown stay as they are. Rejected: Markdown
  code blocks too; code entries only.
- **What counts as a line:** as in ADR 75, a line ends at `\n`, `\r\n` or
  a lone `\r`. In a diff the lines are those of the new text, whose
  unchanged lines take their colours from it (ADR 60); highlighting runs
  from its first line to the last shown unchanged line or to line 50,000,
  whichever comes first.
- **The note:** a dim "no highlighting after line 50,000", in the style
  of the notes of ADR 75, shows where highlighting stops. In a code
  entry it follows line 50,000. In a diff it stands before the first
  unchanged line past line 50,000, never between the removed and the
  added lines of a change, as the side-by-side view faces those lines
  to each other. It shows only when highlighting put colours on the
  lines before it and a line after it shows plain. A text without a
  language, in one patine does not know, or without a coloured token in
  its first 50,000 lines shows no note.
- **One constant:** the 1 MiB of `HIGHLIGHT_LIMIT` is
  `LARGE_ENTRY_BYTES`, which the size warning uses.

## Consequences

- A code entry of 500,000 lines of `x` as Rust takes 0.53 s to build in
  a release build, against 4.92 s of patine alone before. The 1 MiB Rust
  sample takes 1.54 s, and 50,000 short log lines 0.54 s.
- 500,000 lines of `x` as Rust show no note, as no token of `x` takes a
  colour in Rust.
