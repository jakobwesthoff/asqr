# 94. Show a Markdown entry above 1 MiB as plain text

Date: 2026-10-31

## Status

Accepted

Amends [34. Show code and Markdown entries through patine](0034-show-code-and-markdown-entries-through-patine.md):
a Markdown text is rendered only up to 1 MiB

Amends [46. Build and lay out context viewers on worker threads](0046-build-and-lay-out-context-viewers-on-worker-threads.md):
Markdown keeps its highlighted code blocks up to 1 MiB, not at any size

Amends [69. Show a Markdown caption with an image entry](0069-show-a-markdown-caption-with-an-image-entry.md):
a caption above 1 MiB shows as plain text

Amends [75. Cut a text entry after 500,000 lines](0075-cut-a-text-entry-after-500000-lines.md):
its Context sentence that inline text is taken whole, and its
Consequences paragraph on the cost of Markdown, no longer hold for a
Markdown text above 1 MiB

Amended by [98. Show a Markdown entry as plain text when its parser panics](0098-show-a-markdown-entry-as-plain-text-when-its-parser-panics.md)

## Context

- The 500,000-line limit of ADR 75 bounds the memory of the code, ANSI
  and diff viewers, not of the Markdown viewer. ADR 75 measured 8 MiB of
  real Markdown, 173,962 lines, at 693.1 MB and 13.6 s to lay out.
- patine 1.8.0's `render_lines_with`, 80 columns wide, on Markdown files
  joined and cut to each size, in a release build under `/usr/bin/time
  -l` on macOS:

  | Size | Time | Peak memory |
  |---|---|---|
  | 0.5 MiB | 0.16 s | 102.6 MB |
  | 1 MiB | 0.56 s | 233.5 MB |
  | 2 MiB | 1.82 s | 345.9 MB |
  | 4 MiB | 5.46 s | 496.6 MB |
  | 8 MiB | 14.90 s | 796.9 MB |

  Each doubling of the size took about three times as long.
- The Markdown viewer lays out the whole text for the body's width, and
  again whenever the width changes. asqr keeps the viewers of three
  questions at a time (ADR 46), and layout runs on a loading thread.
- A code entry above 1 MiB shows without highlighting, and
  `asqr validate` and `asqr ask` warn about a code, Markdown, ANSI or
  diff entry above 1 MiB.

## Decision

- **Above 1 MiB a Markdown entry shows as plain text:** the text, whole,
  wrapped to the body, below a dim note "over 1 MiB: shown as plain
  text". Nothing is lost, and a code entry above 1 MiB already shows
  plain. Rejected: cutting the text at the limit, and rendering only
  the part in view, which needs a range API in patine and breaks
  references, footnotes and list numbering across blocks.
- **The limit is 1 MiB,** the number the size warning and the
  highlighting limit use. Rejected: 512 KiB, a second number.
- **Inline Markdown counts like a file,** as the viewer costs the same
  for either source and the warning already counts inline text.
  Rejected: keeping inline text whole.
- **An image's caption** follows the same rule, as it is rendered as a
  Markdown entry is (ADR 69).
- **How the size counts:** the text as given, the way the warning counts
  it. Inline, its length; from a file, the bytes read, before the line
  limit of ADR 75 cuts them and before bytes that are not UTF-8 become
  `�`. A text that those steps make longer than 1 MiB shows plain as
  well, so the limit also bounds what patine lays out. A text of blank
  lines only keeps showing "empty", or the notes of its cut.
- **How plain text wraps:** each line wraps at the body's width behind a
  dim `↪`, as a long line of a code block does in rendered Markdown. A
  continuation repeats the blanks the line starts with and no list
  marker, as plain text has no list items. Tabs and control characters
  show as in a code entry.
- **The notes:** the note on plain text comes first, above the notes of
  a cut (ADR 75).
- **The warning:** for a Markdown entry and for a caption it ends
  `asqr shows it as plain text` where it said `asqr still shows it`;
  the warning for the other kinds is unchanged. `WarningKind::LargeEntry`
  carries whether the entry is Markdown.
- **One constant:** `LARGE_ENTRY_BYTES` in `src/format/text_limits.rs`
  holds the 1 MiB the warning and the Markdown viewer use.

## Consequences

- Measured in a release build of the viewer, reading the file through
  `markdown_content` and laying it out 80 columns wide, then 60, with
  patine 1.9.0 for the reference above:

  | Text | First layout | Second layout | Peak memory |
  |---|---|---|---|
  | Markdown joined to 2 MiB | 62 ms | 78 ms | 32.8 MB |
  | Markdown joined to 4 MiB | 137 ms | 175 ms | 51.9 MB |
  | Markdown joined to 8 MiB, 210,543 rows | 236 ms | 315 ms | 88.8 MB |
  | 500,000 lines of `xx`, 1.5 MB | 44 ms | 50 ms | 140.3 MB |

  The peak memory is the footprint of the whole test process.
