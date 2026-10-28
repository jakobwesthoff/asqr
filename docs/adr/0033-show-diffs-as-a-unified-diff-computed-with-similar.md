# 33. Show diffs as a unified diff computed with similar

Date: 2026-10-16

## Status

Accepted

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [49. Present problems through one component in three forms](0049-present-problems-through-one-component-in-three-forms.md)

Amended by [60. Highlight a diff's unchanged lines and fill its changed ones](0060-highlight-a-diffs-unchanged-lines-and-fill-its-changed-ones.md)

Amended by [70. Show a diff side by side on a key](0070-show-a-diff-side-by-side-on-a-key.md):
a diff also shows side by side

Amended by [75. Cut a text entry after 500,000 lines](0075-cut-a-text-entry-after-500000-lines.md)

## Context

A diff entry carries the old and the new text, each inline or from a
file, so asqr computes the diff itself (ADR 28). Diffs get a viewer of
their own behind the pane's viewer interface. The code viewer shows
source lines too, and both have to measure and cut such lines the same
way.

## Decision

- **Unified:** hunks with 3 lines of context, a dim separator line
  between hunks, the old and the new line number in a dim gutter.
- **Line by line:** removed lines red, added lines green, from the
  terminal's palette, so the theme decides the shades. A changed line
  shows whole in its colour, the old version red and the new one green.
  Marked words did not show what changed in the diff of a Rust file,
  so nothing inside a line is marked.
- **New and deleted files:** a missing `old` shows as a new file, a
  missing `new` as a deleted one. `name` goes into the pane's header.
- **Long lines** do not wrap; the viewer reports their full width, so
  the pane scrolls sideways, and a dim `›` marks a line cut at the right
  edge.
- **No syntax highlighting** inside the diff and **no whole-file view**.
- **The crate:** `similar` 3.x, by the author of `insta`, which asqr
  already uses for snapshots, without its `inline` and `unicode`
  features.
- **The sign stays:** each line keeps its `-`, `+` or blank after the
  gutter, so the diff reads without colours too, and in text snapshots.
- **The gutter and the sign stay in place** while the text scrolls
  sideways, so a line's numbers stay in sight.
- **The separator** is a dim `┈` across the body. The jump in the line
  numbers tells how many lines lie between two hunks.
- **Notes:** a dim "new file" or "deleted file" heads such a diff,
  "empty file" follows when that file has no text, and two equal sides
  show "no changes".
- **Missing newline:** a dim "no newline at end of file" follows a last
  line without a newline only when the other side ends with one. Then
  the removed and the added line look the same and need the note.
  When both sides lack it, the note would only repeat on every diff of
  inline text, which agents often write without a final newline.
- **Line ends:** both sides have `\r\n` and a lone `\r` turned into `\n`
  before they are compared, for the check for equal sides as well as
  for the diff. Otherwise a file saved on Windows, diffed against its
  Unix copy, shows every line removed and added again with the same
  text, and nothing says why. A dim note says it instead, once: above
  the hunks, or below "no changes". A side of a single line without a
  line end has no line ends to differ in.
- **The note names the ends** of both sides: `line endings differ: old
  CRLF, new LF`. `CRLF` is `\r\n`, `LF` is `\n` and `CR` a lone `\r`. A
  side with ends of several kinds is `mixed` with each kind named, as
  `old mixed CRLF and LF`, in the order CRLF, LF, CR; a side with all
  three is `mixed CRLF, LF and CR`. In a narrow pane the note is cut at
  the edge, as notes are. A note owns its text where the text is built
  for the content, as this one is.
- **Histogram:** the diff uses `similar`'s Histogram algorithm, git's
  algorithm of that name, which anchors on lines that occur rarely,
  such as a function's signature, rather than on blank lines and lone
  braces.
- **A time limit:** the person waits for the diff of the entry shown,
  which a worker builds (ADR 46), so computing the line diff may take
  one second. Past that, `similar` returns a diff that is still correct
  but may mark more lines as changed than needed.
- **Reading files:** a relative path is never resolved, as for images.
  A file with a NUL byte counts as binary, as git decides, and is not
  shown. Bytes that are not UTF-8 show as `�`, so one stray byte from
  another encoding leaves the rest readable. A side that cannot be read
  shows as the viewer's content, a card that names the side and the
  reason (ADR 49), for the first side that fails.
- **Only regular files, and at most 8 MiB of each:** the person waits
  for the entry shown. Opening a FIFO would wait for a writer, and a
  device such as `/dev/zero` would never end, so a path that is not a
  regular file is refused before it is opened, with "not a regular
  file", and the open itself does not wait on a FIFO (ADR 46). A file is
  read up to 8 MiB, and a longer one shows its start under a dim "first
  8 MiB shown" and above a dim "… the rest is cut after 8 MiB", as the
  last hunk would otherwise look like the end of the file to someone who
  scrolled there.
- **A cut between CR and LF:** a side whose 8 MiB limit falls between
  the `\r` and the `\n` of a `\r\n` must not end in a lone `\r`.
  Otherwise the note names a `CR` end the file does not use: `old mixed
  CRLF and CR` against an LF side, and a note against a CRLF side, where
  none belongs. When the limit falls inside a `\r\n`, the text stops
  before the `\r`. The read takes one byte past the limit to tell a cut
  file from a whole one, and that byte tells such a `\r` from a lone
  one, which stays. A blank line whose line end straddles the limit is
  then left out, as if the limit fell one byte earlier, right after the
  `\r\n` before it. The rule lives in `src/tui/render/source_file.rs`, so
  the code and Markdown viewers (ADR 34) read the same way. Dropping a
  trailing `\r` in the diff instead would not see the byte past the
  limit, and would also drop the last line end of a CR file cut right
  after one.

For every viewer that shows lines of source:

- **Tabs** expand to the next stop of 4 columns, counted from the start
  of the line. Panes are often 40 to 60 columns wide, where the
  terminal's 8 would push indented code out of sight.
- **Control characters** other than tab show as their pictures from
  Unicode's Control Pictures block, such as `␛` for escape, and the C1
  controls, which have none, as `�`. ratatui leaves control characters
  out of the cells it draws, which would put a line's measured width
  out of step with what shows, and an escape that reached the terminal
  could change the screen.
- **Cut lines:** a line that goes on past the right edge ends in a dim
  `›`, and a line scrolled sideways past text that is not blank starts
  with a dim `‹`, so the person sees which lines have more on either
  side. Blanks alone, such as an indent, get no `‹`. Each marker takes
  the column it sits on, and a wide character that would cross into a
  marker's cell or past an edge shows as blanks in its style. A marker
  written into the trailing half of a wide character would not show.
- **Columns:** `columns` and `fitting` in
  `src/tui/render/source_line.rs` measure a text grapheme by grapheme
  through ratatui's cell width, and every width the screen lays out goes
  through them, so what the layout counts is what ratatui draws. A
  halfwidth sound mark such as `ﾞ` therefore takes a column of its own,
  as ratatui and terminals draw it.
- One module, `src/tui/render/source_line.rs`, does all of this; the
  code and Markdown viewers of ADR 34 use it too.

## Consequences

- The person sees a change as a diff, the format a code review uses,
  without the asker writing one.
- asqr depends on `similar` 3.2. `insta` still depends on 2.7, so the
  lockfile has both; `insta` is a development dependency, so the binary
  carries 3.2 alone. `unicode-segmentation` is a direct dependency, at
  the version ratatui uses, for measuring columns.
- A diff of two large files may take a second before the pane shows it.
