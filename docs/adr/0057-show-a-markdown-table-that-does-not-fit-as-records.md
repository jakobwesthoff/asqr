# 57. Show a Markdown table that does not fit as records

Date: 2026-10-22

## Status

Accepted

Amends [34. Show code and Markdown entries through patine](0034-show-code-and-markdown-entries-through-patine.md)

Amends [35. Render the text fields as full Markdown through patine](0035-render-the-text-fields-as-full-markdown-through-patine.md)

## Context

With patine 1.7.2, a word wider than its table column broke inside its
cell, so a table fit its width (ADR 34). A table with more columns than
its width holds shrank them to single letters: at a width of 22, five
columns of short words broke into one letter per line. Below one
character per column the table overflowed, and `markdown_rows` wrapped
each of its rows and repeated its border `│ ` as if it were a quote
bar: twelve one-letter columns need 49 columns, so at a width of 20
every border and row wrapped.

patine counted a halfwidth sound mark such as `ﾞ` as no column and asqr
as one (ADR 33), so a table row holding one was wider on screen than
patine laid it out. A cell of `ｶﾞ` six times at a width of 12 drew
`│ x │ ｶﾞｶﾞｶﾞ` and then `│ ↪ ｶﾞ │`.

patine 1.8.0 lays out such a table as records and measures widths
grapheme by grapheme.

## Decision

- **Records:** a table that does not fit shows as records, one line per
  column, rather than wrapping its rows without the false quote bar or
  cutting it with sideways scrolling. patine lays the records out, and
  asqr draws its lines as they come.
- **As soon as a word would break:** a table shows as records as soon
  as a column would break words, not only once the table overflows. A
  URL counts as a word. A word never breaks inside a boxed table.
- **The record:** each data row is a record, and each column a line of
  it: the column's name from the header row in bold and dim, then the
  row's cell. A plain rule across the width separates two records. A
  table without data rows is one record of its names. The values stand
  beside the names, and a long value wraps there; where that room would
  break a word, every value of the table stands under its name,
  indented two columns.
- **Alignment** from the delimiter row applies to boxed tables only.
- **Widths:** patine counts a halfwidth sound mark as a column of its
  own, and asqr does not correct it. patine measures every width
  grapheme by grapheme, so its widths equal those of asqr's `columns`
  apart from control characters, which patine counts as one column and
  `columns` as none; `SourceLine` shows a control character as its
  picture in one column. Tests check that both agree for `لا` and `ｶﾞ`.
  asqr measures patine's lines with `columns` and `SourceLine`, never
  with ratatui's `Span::width` or `Line::width`, which measure a whole
  string and differ for such text.
- **What still wraps:** `markdown_rows` wraps every line wider than its
  space, as before. A line of a table reaches it only where one
  grapheme, such as a wide character, is wider than the room a record
  gives it, or where quote bars and indents leave the table no column.
  This replaces ADR 34's **Tables** bullet and takes the table that
  cannot shrink out of the lists of lines that wrap in ADR 34's
  **Markdown never cuts a line** and ADR 35's **Nothing is cut**.

## Consequences

- asqr depends on patine 1.8.0 or later.
- A boxed table is at least as wide as the widest word of each of its
  columns, a link's `(url)` included, plus its borders, so a table with
  a link in a cell shows as records in any width narrower than that:
  the table of the tests, whose URL takes 54 columns, from 65 columns
  down.
