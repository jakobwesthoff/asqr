# 70. Show a diff side by side on a key

Date: 2026-10-28

## Status

Accepted

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md):
the focused pane, the full-screen view and the question's rows take `s`

Amends [33. Show diffs as a unified diff computed with similar](0033-show-diffs-as-a-unified-diff-computed-with-similar.md):
a diff also shows side by side

Amends [60. Highlight a diff's unchanged lines and fill its changed ones](0060-highlight-a-diffs-unchanged-lines-and-fill-its-changed-ones.md):
side by side, a changed line fills its column

## Context

A diff entry carries the old and the new text (ADR 28), and asqr
computes the diff and shows it unified (ADR 33): a changed line shows as
its old version in a red row and its new version in a green row below
it, unchanged lines highlighted in the diff's language (ADR 60). A pane
or a full-screen view with room for two columns of code can show the two
versions next to each other instead.

## Decision

- **Unified first:** a diff shows unified, as before. Whether side by
  side reads better is not settled, so the unified diff stays what shows
  first and side by side is one key away.
- **The key:** `s` switches the diff shown between unified and side by
  side. It works wherever `z` and `o` work: in the focused pane, in the
  full-screen view and on the question's rows, not while a field or a
  note takes the keys, where it types. The person switches while
  scrolling and looking back and forth, without moving the focus into
  the pane first. `s` rather than `d`, as `d` sits next to `ctrl-d`,
  which scrolls.
- **How long:** the switch holds for that entry while it shows, as the
  full-screen view does. Once another entry shows, it ends, and the diff
  shows unified when it comes back. A replaced session file keeps it
  while the same entry shows, as it keeps the full-screen view. asqr
  does not keep it when it quits, and no setting makes side by side the
  default.
- **Narrow panes:** side by side is allowed wherever two columns can be
  drawn at all; whether they read well there is the person's call. Two
  columns need each its gutter and at least 8 columns of text, and the
  divider with its blank. Below that the diff shows unified and `s` says why: "too
  narrow for side by side; z shows it full screen", in the full-screen
  view "too narrow for side by side". `s` back to unified always works.
- **Beside the one-line hint,** where no pane shows, `s` switches the
  diff and says "side by side shows with z", as the switch belongs to
  the entry.
- **Other refusals:** "only a diff shows side by side" on another
  entry, "this diff cannot be shown side by side" on a diff with a side
  that cannot be read (ADR 49), and "nothing to show side by side here"
  where no entry shows.
- **The columns:** the old text left, the new text right, a dim `│`
  and a blank between them, so a line number of the new column never
  touches the line; the left column takes the smaller half of what they
  leave.
  Each column has its own gutter: its side's line number, right-aligned
  to the digits both sides share, and the sign. An unchanged line shows
  on both sides, its old number left and its new number right.
- **Alignment:** in a run of changed lines the k-th removed line faces
  the k-th added one, whatever order the diff lists them in, and blank
  rows face the rest of the longer side. "no newline at end of file"
  takes a row of its own under its line, in that line's column, facing
  a blank. The separator between hunks and every other note span both
  columns.
- **Sideways:** both columns scroll together, as far as the line that
  runs furthest past its column needs, and each column marks its cut
  lines with its own `‹` and `›`.
- **The line at the top stays:** the rows of the two layouts differ, so
  an offset kept as a row number would show another line in the other
  layout. An entry's offset counts the rows of the layout it was last
  drawn in. Drawn in the other one, it shows from the row that holds
  the same line, and that row becomes its offset, with the row it came
  from, so switching back before scrolling returns to the same line.
- **What stays from ADRs 33 and 60:** hunks with 3 lines of context, the
  line numbers, the signs, the red and green fills of changed lines,
  which side by side run across their column, unchanged lines
  highlighted from the new text, and nothing marked inside a line.
- **The key bar and the help:** while the focused pane or the
  full-screen view shows a diff that `s` can switch, the key bar names
  `s  side by side`, or `s  unified`, before `?  help`. The help of the
  pane and of the full-screen view lists `s  a diff side by side or
  unified`; the general help names `s` on the line of `o` and `z`, as
  it has room for 13 lines on the smallest screen.
- **Switching lays the viewer out again,** as a resize does, behind the
  pane's wait for a viewer (ADR 46); the diff is computed and
  highlighted once, when the viewer is built, for both layouts. Only a
  diff takes the layout, so a switch never lays out another entry
  again.

## Consequences

- A diff's viewer holds the rows of both layouts from the time it is
  built.
- The general help's line of `o` and `z` reads `o, z, s  open an image;
  full screen; diff side by side`.
