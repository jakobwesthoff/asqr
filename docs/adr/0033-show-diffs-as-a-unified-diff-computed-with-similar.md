# 33. Show diffs as a unified diff computed with similar

Date: 2026-10-14

## Status

Accepted

Amends [28. Session and result format version 2 with a context field](0028-session-and-result-format-version-2-with-a-context-field.md)

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

## Context

A diff entry carries the old and the new text, each inline or from a
file, so asqr computes the diff itself (ADR 28). Until now the context
pane showed a placeholder for it (ADR 31). Step 5 of the plan todo
"Split the TUI into a question pane and a context pane" gives diffs a
viewer of their own behind the pane's viewer interface. The text viewer
of the next step shows source lines too, and both have to measure and
cut such lines the same way.

## Decision

Taken with the user (plan, *Viewers*, and the step's todo):

- **Unified:** hunks with 3 lines of context, a dim separator line
  between hunks, the old and the new line number in a dim gutter.
  Side by side follows in its own todo.
- **Colours:** removed lines red, added lines green, from the terminal's
  palette, so the theme decides the shades. Inside a changed line, the
  changed words are emphasised on top of the colour.
- **New and deleted files:** a missing `old` shows as a new file, a
  missing `new` as a deleted one. `name` goes into the pane's header.
- **Long lines** do not wrap; the viewer reports their full width, so
  the pane scrolls sideways, and a dim `›` marks a line cut at the right
  edge.
- **No syntax highlighting** inside the diff and **no whole-file view**
  in this phase.
- **The crate:** `similar` 3.x, by the author of `insta`, which asqr
  already uses for snapshots. Its `inline` feature finds the changed
  words.

Agent's choices within that design:

- **Words end at punctuation:** `similar`'s `unicode` feature splits a
  line into Unicode words, so `get(url)` changed to `get(uri)` marks
  `url` and `uri` and not the whole call. Without it, words are runs
  between blanks. The feature pulls in `unicode-segmentation`, which
  ratatui already depends on. `similar`'s semantic cleanup is on, which
  shifts the marked ranges so they read better.
- **Emphasis is reversed video:** a changed word shows in its line's
  colour, reversed. It stays visible on a terminal without colours.
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
- **Histogram:** the diff uses `similar`'s Histogram algorithm, git's
  algorithm of that name, which anchors on lines that occur rarely,
  such as a function's signature, rather than on blank lines and lone
  braces.
- **A time limit:** `load` runs while the person waits for the screen,
  so computing a diff, words included, may take one second. Past that,
  `similar` returns a diff that is still correct but may mark more lines
  as changed than needed.
- **Reading files:** a relative path is never resolved, as for images.
  A file with a NUL byte counts as binary, as git decides, and is not
  shown. Bytes that are not UTF-8 show as `�`, so one stray byte from
  another encoding leaves the rest readable. A side that cannot be read
  shows as the viewer's content: "diff not shown", its path and the
  reason, for the first side that fails.

Agent's choices, confirmed in the review of this step's plan, for every
viewer that shows lines of source:

- **Tabs** expand to the next stop of 4 columns, counted from the start
  of the line. Panes are often 40 to 60 columns wide, where the
  terminal's 8 would push indented code out of sight.
- **Control characters** other than tab show as their pictures from
  Unicode's Control Pictures block, such as `␛` for escape, and the C1
  controls, which have none, as `�`. ratatui leaves control characters
  out of the cells it draws, which would put a line's measured width
  out of step with what shows, and an escape that reached the terminal
  could change the screen.
- **Cut lines:** a wide character cut by either edge of the visible
  columns shows as blanks in its style, and a line that goes on past the
  right edge ends in a dim `›`.
- One module, `src/tui/render/source_line.rs`, does all three, so the
  text viewer takes it over unchanged.

## Consequences

- The person sees a change as a diff, the format a code review uses,
  without the asker writing one.
- asqr depends on `similar` 3.2. `insta` 1.49 still depends on 2.7, so
  the lockfile has both; `insta` is a development dependency, so the
  binary carries 3.2 alone. `unicode-segmentation` and `unicode-width`
  become direct dependencies at the versions ratatui already uses.
- A diff of two large files may take a second before the pane shows it.
- Text entries still show their placeholder until the text viewer lands.
