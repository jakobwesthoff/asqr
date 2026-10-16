# 33. Show diffs as a unified diff computed with similar

Date: 2026-10-14

## Status

Accepted

Amends [28. Session and result format version 2 with a context field](0028-session-and-result-format-version-2-with-a-context-field.md)

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [49. Present problems through one component in three forms](0049-present-problems-through-one-component-in-three-forms.md)

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
  changed words are emphasised on top of the colour. The amendment of
  2026-10-16 drops the emphasis.
- **New and deleted files:** a missing `old` shows as a new file, a
  missing `new` as a deleted one. `name` goes into the pane's header.
- **Long lines** do not wrap; the viewer reports their full width, so
  the pane scrolls sideways, and a dim `›` marks a line cut at the right
  edge.
- **No syntax highlighting** inside the diff and **no whole-file view**
  in this phase.
- **The crate:** `similar` 3.x, by the author of `insta`, which asqr
  already uses for snapshots. Its `inline` feature finds the changed
  words. The amendment of 2026-10-16 drops the `inline` feature.

Agent's choices within that design:

- **Words end at punctuation:** `similar`'s `unicode` feature splits a
  line into Unicode words, so `get(url)` changed to `get(uri)` marks
  `url` and `uri` and not the whole call. Without it, words are runs
  between blanks. The feature pulls in `unicode-segmentation`, which
  ratatui already depends on. `similar`'s semantic cleanup is on, which
  shifts the marked ranges so they read better. Dropped by the
  amendment of 2026-10-16.
- **Emphasis is reversed video:** a changed word shows in its line's
  colour, reversed. It stays visible on a terminal without colours.
  Dropped by the amendment of 2026-10-16.
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
  text, and nothing says why. A dim "line endings differ" says it
  instead, once: above the hunks, or below "no changes". A side of a
  single line without a line end has no line ends to differ in. The
  amendment of 2026-10-16 (naming the line ends) names the ends.
- **Histogram:** the diff uses `similar`'s Histogram algorithm, git's
  algorithm of that name, which anchors on lines that occur rarely,
  such as a function's signature, rather than on blank lines and lone
  braces.
- **A time limit:** `load` runs while the person waits for the screen,
  so computing a diff, words included, may take one second. Past that,
  `similar` returns a diff that is still correct but may mark more lines
  as changed than needed. Since the amendment of 2026-10-16 there are
  no words to compute.
- **Reading files:** a relative path is never resolved, as for images.
  A file with a NUL byte counts as binary, as git decides, and is not
  shown. Bytes that are not UTF-8 show as `�`, so one stray byte from
  another encoding leaves the rest readable. A side that cannot be read
  shows as the viewer's content: "diff not shown", its path and the
  reason, for the first side that fails.
- **Only regular files, and at most 8 MiB of each:** `load` runs while
  the person waits for the screen. Opening a FIFO would wait for a
  writer, and a device such as `/dev/zero` would never end, so a path
  that is not a regular file is refused before it is opened, with "not
  a regular file". A file is read up to 8 MiB, and a longer one shows
  its start under a dim "first 8 MiB shown" and above a dim "… the rest
  is cut after 8 MiB", as the last hunk would otherwise look like the
  end of the file to someone who scrolled there. Reading off the drawing
  thread would keep a large file from stalling the screen at all; it is
  not part of this decision.

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
- **Cut lines:** a line that goes on past the right edge ends in a dim
  `›`, and a line scrolled sideways past text that is not blank starts
  with a dim `‹`, so the person sees which lines have more on either
  side. Blanks alone, such as an indent, get no `‹`. Each marker takes
  the column it sits on, and a wide character that would cross into a
  marker's cell or past an edge shows as blanks in its style. A marker
  written into the trailing half of a wide character would not show.
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

## Amendment (2026-10-14, measuring in terminal columns)

`unicode-width` is no longer a direct dependency. `columns` and
`fitting` in `src/tui/render/source_line.rs` measure a text grapheme by
grapheme through ratatui's cell width, and every width the screen lays
out goes through them, so what the layout counts is what ratatui draws.
A halfwidth sound mark such as `ﾞ` therefore takes a column of its own,
as ratatui and terminals draw it.

## Amendment (2026-10-16, a line diff without marked words)

The user tested the diff of a Rust file before and after a change on
2026-10-16 and compared the marking as built with three other ways of
marking words. The marked words did not show what changed, and none of
the ways convinced the user, who decided that asqr shows diffs line by
line without marked words.

- A changed line shows whole in its colour: the old version red, the
  new one green. Nothing inside a line is reversed. This replaces the
  decision that the changed words are emphasised on top of the colour,
  and the agent's choices *Words end at punctuation* and *Emphasis is
  reversed video*.
- `similar` comes without its `inline` and `unicode` features.
  `unicode-segmentation` stays a direct dependency, for measuring
  columns.
- The time limit of one second bounds the line diff alone.

How GitHub and other tools pair lines and mark words is left to the
todo `01M520ZWWR3RJAGXT9VDZ36DZK-find-out-how-github-marks-the-changed-words-of-a-diff.md`.

## Amendment (2026-10-16, naming the line ends)

The user, testing a diff of a CRLF file against an LF file on
2026-10-16, asked that the note say which ends each side uses, and
decided its wording:

- The note names the ends of both sides: `line endings differ: old
  CRLF, new LF`. `CRLF` is `\r\n`, `LF` is `\n` and `CR` a lone `\r`.
- A side with ends of several kinds is `mixed` with each kind named,
  as `old mixed CRLF and LF`. In a narrow pane the note is cut at the
  edge, as notes are.

Agent's choices: the kinds are named in the order CRLF, LF, CR, and a
side with all three is `mixed CRLF, LF and CR`. A note owns its text
where the text is built for the content, as this one is.
