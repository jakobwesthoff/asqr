# 75. Cut a text entry after 500,000 lines

Date: 2026-10-28

## Status

Accepted

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
`validate` and `ask` warn about a text entry of more than 500,000 lines

Amends [33. Show diffs as a unified diff computed with similar](0033-show-diffs-as-a-unified-diff-computed-with-similar.md):
each side of a diff is cut after 500,000 lines as well as after 8 MiB

Amends [34. Show code and Markdown entries through patine](0034-show-code-and-markdown-entries-through-patine.md):
a code or Markdown text is cut after 500,000 lines as well as after
8 MiB

Amends [56. Show text with ANSI colours as a context entry](0056-show-text-with-ansi-colours-as-a-context-entry.md):
an `ansi` text is cut after 500,000 lines, which bounds the memory its
viewer keeps

Amends [69. Show a Markdown caption with an image entry](0069-show-a-markdown-caption-with-an-image-entry.md):
a caption is cut after 500,000 lines

Amended by [94. Show a Markdown entry above 1 MiB as plain text](0094-show-a-markdown-entry-above-1-mib-as-plain-text.md)

## Context

- A viewer reads at most the first 8 MiB of a file (ADR 33, ADR 34,
  ADR 56); inline text is taken whole. The 8 MiB limit bounds the bytes
  read, not the memory of what the viewer keeps (ADR 56).
- The code, ANSI and diff viewers keep every line as a row of its own.
  asqr keeps the viewers of three questions at a time (ADR 46).
- Peak memory, measured as below, to build the viewer of a file of
  8 MiB of line breaks (8,388,608 empty lines): 828.0 MB in the code
  viewer, 559.4 MB in the ANSI viewer, 1,837 MB in the diff of a new
  file of that text.
- A text of millions of lines looks like an error or misuse rather than
  something to optimise for.
- Lines in 8 MiB of real files on a development machine (files of at
  least 64 KiB, up to 3,000 a kind), all bytes over all lines, and the
  densest tenth of files:

  | Kind | Lines in 8 MiB | Densest tenth |
  |---|---|---|
  | Rust | 177,708 | 256,373 |
  | C and headers | 227,750 | 304,449 |
  | JavaScript and TypeScript, not minified | 176,090 | 279,503 |
  | Minified JavaScript | 18,140 | 15,464 |
  | Logs | 65,322 | 201,988 |
  | `package-lock.json` | 231,949 | 246,144 |
  | `Cargo.lock` | 338,843 | 347,078 |
  | Markdown | 190,759 | 288,932 |

  Only generated tables, at 13 to 18 bytes a line, held 470,000 to
  640,000 lines in 8 MiB.

## Decision

- **The limit:** a viewer keeps at most the first 500,000 lines of a
  text, beside the 8 MiB limit. It cuts no ordinary file of 8 MiB, the
  densest of which hold about 350,000 lines, and it bounds the worst
  case at the limit, 500,000 lines of 16 bytes that come close to
  8 MiB, to 154.1 MB in the code viewer and 85.7 MB in the ANSI viewer,
  against 828.0 MB and 559.4 MB before.
- **Measured how:** a test build in release mode built one viewer from
  a file through the viewers' own reading path and laid it out 80
  columns wide; each probe ran alone under `/usr/bin/time -l` on macOS,
  which reports the peak memory footprint. The probe binary alone,
  writing its 8 MiB file and building nothing, took 11.8 MB.
- **Every text entry:** `code`, `markdown`, `ansi` and both sides of a
  `diff`, inline or from a file, and an image entry's caption. For a
  file, the line limit applies to what the 8 MiB limit leaves.
- **What a line is:** a line ends at `\n`, `\r\n` or a lone `\r`, as
  patine and the diff end them, and in an ANSI entry only at `\n`, as
  its reader keeps a lone `\r` in the line. The text is cut after the
  line end of its 500,000th line when anything follows, an empty line
  included.
- **The notes:** a dim "first 500,000 lines shown" heads the content
  and a dim "… the rest is cut after 500,000 lines" ends it, like the
  notes of the 8 MiB limit. A file that both limits cut shows the line
  notes, as the line limit ends the text shown.
- **A diff:** each side counts on its own, as each side is read up to
  8 MiB on its own, and the rest of the other side shows as removed or
  added. The notes do not name the side. Sides cut by different limits
  show the notes of both, those of 8 MiB first.
- **Blank text:** a Markdown text or a caption whose kept lines are all
  blank shows only the two notes rather than "empty", as more text
  follows. It is not rendered: patine took 40 s and 418 MB over
  500,000 blank lines.
- **Memory of the cut:** a file's bytes are cut before they are
  decoded, and inline text copies only the lines kept, so a cut text
  holds no memory for the rest.
- **The warning:** `asqr validate` and `asqr ask` warn about a text
  entry, a side of a diff or a caption of more than 500,000 lines, as
  they warn above 1 MiB: `more than 500,000 lines; asqr shows only the
  first 500,000`, at the side's or the caption's path for those. They
  count lines as the viewer does: inline text whole, a file in its
  first 8 MiB, and a file with a NUL byte, which the viewer does not
  show, not at all. A file of no more than 500,000 bytes cannot hold
  more lines and is not read. An entry above both limits gets both
  warnings, the 1 MiB one first, which keeps its text.
- Lines are not made leaner in memory.

## Consequences

- After the change, 8 MiB of line breaks takes 64.1 MB in the code
  viewer, 46.7 MB in the ANSI viewer, 118.7 MB in the diff of a new
  file and 13.9 MB in the Markdown viewer.
- `asqr validate` of a session whose code entry is a file of 8 MiB of
  real Rust took 0.01 s and a peak of 13.7 MB, the read for the line
  count included.
- The limit bounds memory, not time: a code entry under 1 MiB is still
  highlighted, and 500,000 lines of `x` in Rust took 4.47 s.
- Markdown's cost grows with its bytes rather than its lines: 8 MiB of
  real Markdown, 173,962 lines, took 693.1 MB and 13.6 s to lay out,
  and the line limit does not change that.
