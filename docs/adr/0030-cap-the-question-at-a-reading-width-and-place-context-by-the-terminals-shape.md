# 30. Cap the question at a reading width and place context by the terminal's shape

Date: 2026-10-16

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md)

Amended by [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md)

Amended by [47. Put the session's title in the title bar](0047-put-the-sessions-title-in-the-title-bar.md)

Amended by [52. Scroll half a page with ctrl-u and ctrl-d](0052-scroll-half-a-page-with-ctrl-u-and-ctrl-d.md):
`ctrl-u` and `ctrl-d` scroll the intro overlay by half a page

Amended by [54. Set the context pane off from the question with a ledge](0054-set-the-context-pane-off-from-the-question-with-a-ledge.md):
beside the question the pane has a left edge, and below it a blank row
stands between question and pane

Amended by [58. Name ctrl-u and ctrl-d in the general help and the key bar](0058-name-ctrl-u-and-ctrl-d-in-the-general-help-and-the-key-bar.md):
the intro overlay's key bar names `ctrl-u/ctrl-d`

## Context

Over the full width of a terminal of about 230 columns, question texts,
option descriptions and notes run across nearly all of it and are hard
to read. The context area goes beside or below the question.

## Decision

- **Reading width:** the question never gets wider than a reading width
  and starts at the left, with or without context. The reading width is
  100 columns. The review tab keeps to the reading width too.
- **Beside:** the question takes half the width until it reaches the
  reading width, and the context area gets the rest after a one-column
  separator. It needs at least 60 columns for the question and 40 for
  the context.
- **Below:** the question takes the rows the tallest question with
  context in the session needs, at most half the height, and the context
  area the rest, over the whole width. It needs at least 8 rows for the
  context.
- **Automatic choice:** `--layout auto`, the default, measures the
  visible shape W ÷ 2H of the tab's area, as cells are about twice as
  tall as wide. When both arrangements fit, a landscape area (at least
  1) gets the context beside and a portrait one below. On a resize,
  beside starts from 1.1 and below from under 0.9. When only one fits,
  that one is used.
- **Override:** `--layout beside` and `--layout below` override the
  choice, and the environment variable `ASQR_LAYOUT` does the same, in
  the style of ADR 15.
- **No room:** when neither fits, the context area shrinks to a one-line
  hint. `z` shows the entry full screen.
- **Without context:** a question without context takes the width it
  can get, up to the reading width, and its text reflows when moving to
  a question with context beside.
- **Memory:** a redraw without a resize keeps the current arrangement.
- **Intro overlay:** the header shows a cut intro (ADR 47), and `i`
  shows the whole intro in an overlay. The overlay never cuts the intro.
  A longer one scrolls with the keys of the context pane: `↑`/`↓` and
  `j`/`k` by line, `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` by page, `g`/`G`
  and `home`/`end` to the ends. `esc`, `q` and `i` close it, and other
  keys do nothing, so a scroll key never closes it. A dim position such
  as `4–15 of 16` on its bottom edge shows where it stands, and the key
  bar, which the overlay leaves free, says what scrolls and closes it.
  `ctrl-c` quits over the intro overlay, as it does everywhere but over
  the help and the dialog of an invalid session (ADR 49). On a session
  without an intro, `i` does nothing. The help lists `i` on the line of
  `?`, as the help fills the 15 rows of the smallest screen.
- W×H is the tab's area inside the one-column margins, so beside needs
  W = 101, a terminal of 103 columns. The question beside a context
  area takes half of W after the separator, at least 60 and at most 100
  columns.
- Below fits when the context area gets 8 rows with the question at
  half the height, from H = 15, whatever the question needs. A question
  that grows while the person types never switches the arrangement.
- A layout given by flag or variable that does not fit gives way to the
  other one when that fits, and to the hint otherwise.
- The screen's `View`, which is kept from one draw to the next, holds
  the last arrangement shown beside or below. A hint leaves it as it
  is, so after a terminal that was too small for either, the
  hysteresis continues from what was shown before.
- `ASQR_LAYOUT` is read only by the TUI. An empty value counts as
  unset, and any other value than `auto`, `beside` or `below` is a usage
  error when asqr watches the queue, while the other commands ignore
  it. A `--layout` after `watch` wins over one before it, and any flag
  wins over the variable.
- The hint names the entry by its `title`, or by the file name of its
  path (a diff by its `name`), and adds `z: full screen` (ADR 31).

The height of the question below the context:

- **Per session:** below, a question that took the rows its own content
  needs would make the context pane jump up and down while the person
  moves between questions of different heights, and move with every
  line typed. Beside the question, a narrower pane only reflows its
  text, so a fixed width costs nothing. Below, a fixed height would hide
  rows and force the person to scroll, so the height follows what the
  questions need, but per session, so the pane does not jump between
  questions.
- **Per line while typing:** what the person has typed counts. The
  split grows as soon as the tallest question gains a row and shrinks
  only once no field or note has the keys, so it moves at most once per
  new line and never while a line is deleted.
- A question's rows are counted by the function that draws it, drawn
  without a cursor: every field unfocused with the text typed so far, a
  row kept for the counter of a field with a length, and the cursor's
  blank after a full line left out. A second way to count would drift
  from the drawing.
- While a field or a question's note has the keys, the current question
  counts as drawn with its text cursor. Counted without it, a text that
  fills its line exactly would hide the question's last row behind
  `↓ more` until the next key, as the cursor's blank takes a row the
  split does not count. The other questions are still counted without a
  cursor.
- Questions without context do not count, as they show no pane.
- The screen's `View` keeps the rows of every question by session,
  revision of the session file, question and width, and counts the
  current question on every draw. The high mark of the split starts
  afresh on a resize, in another session and after the file was
  replaced. Moving the focus into the context pane counts as leaving
  the field.
- A text question gives its field the keys as soon as it shows, so on
  one the split moves back up only after `esc` or `tab`.

## Consequences

- Terminals from 103 columns on show the context beside the question
  when their tab area is landscape; 120×40 and 230×60 do.
- A tab area narrower than 101 columns and lower than 15 rows, such as
  on an 80×20 terminal, shows the context as a one-line hint.
- Below, a question taller than half the tab area scrolls inside it.
  On a 60×24 terminal the example session `alt-rework-batch.json` shows
  its own answer and its note only when the cursor reaches them.
