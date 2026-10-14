# 30. Cap the question at a reading width and place context by the terminal's shape

Date: 2026-10-14

## Status

Accepted

Amends [8. TUI interaction model](0008-tui-interaction-model.md)

Amends [22. Answer in place with a tab bar and a review step](0022-answer-in-place-with-a-tab-bar-and-a-review-step.md)

## Context

ADR 22 laid the TUI out in one column over the full width. On a
terminal of about 230 columns, question texts, option descriptions and
notes ran across nearly all of it and were hard to read (user's
screenshot, 2026-10-14). Context went beside the question at 3:2 from
100 columns of content on, and below it otherwise, in a strip of a third
of the height and at most 12 rows.

The user settled a new layout on 2026-10-14, recorded in the plan todo
"Split the TUI into a question pane and a context pane". This ADR covers
its second step: the reading width and where the context area goes. The
context area becomes a pane with focus and scrolling in the next step.

## Decision

Taken with the user:

- **Reading width:** the question never gets wider than a reading width
  and starts at the left, with or without context. The plan names about
  100 to 110 columns, to be tuned by trying it; asqr starts with 100.
- **Beside:** the question takes half the width until it reaches the
  reading width, and the context area gets the rest after a one-column
  separator. It needs at least 60 columns for the question and 40 for
  the context.
- **Below:** the question takes the rows it needs, up to half the
  height, and the context area the rest, over the whole width. It needs
  at least 8 rows for the context.
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
  hint. `z` shows an image full screen as before.
- **Without context:** a question without context takes the width it
  can get, up to the reading width, and its text reflows when moving to
  a question with context beside. The user left this choice to the
  agent.
- **Where the memory lives:** left to the agent, as long as a redraw
  without a resize keeps the current arrangement.
- **Header:** the session title and intro wrap at the reading width; the
  tab bar keeps the full width. The intro still shows at most three
  lines, the cut is marked `… i: more`, and `i` shows the whole intro in
  an overlay. The skill explains what the intro is for.

Settled while building it:

- **Intro overlay:** it never cuts the intro. A longer one scrolls with
  the keys the plan gives the context pane: `↑`/`↓` and `j`/`k` by line,
  `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` by page, `g`/`G` and `home`/`end`
  to the ends. `esc`, `q` and `i` close it, and other keys do nothing,
  so a scroll key never closes it. A dim position such as `4–15 of 16`
  on its bottom edge shows where it stands, and the key bar, which the
  overlay leaves free, says what scrolls and closes it.

Agent's choices within that design:

- W×H is the tab's area inside the one-column margins, so beside needs
  W = 101, a terminal of 103 columns. The question beside a context
  area takes half of W after the separator, at least 60 and at most 100
  columns.
- Below fits when the context area gets 8 rows with the question at
  half the height, from H = 15, whatever the question needs. A question
  that grows while the person types never switches the arrangement.
- A layout given by flag or variable that does not fit gives way to the
  other one when that fits, and to the hint otherwise.
- The review tab keeps to the reading width too.
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
  path (a diff by its `name`), and adds `z: full screen` for an image.
- The title takes at most two lines, one below 24 rows, and a cut title
  ends in `…`.
- `ctrl-c` quits over the intro overlay, as it does everywhere but over
  the help. On a session without an intro, `i` does nothing.
- The help lists `i` on the line of `?`. The help already filled the
  15 rows of the smallest screen.

## Consequences

- Terminals from 103 columns on show the context beside the question
  when their tab area is landscape; 120×40 and 230×60 do.
- A tab area narrower than 101 columns and lower than 15 rows, such as
  on an 80×20 terminal, shows the context as a one-line hint, where the strip of a
  third of the height showed it before.
- Below, a question taller than half the tab area scrolls inside it.
  On a 60×24 terminal the example session `alt-rework-batch.json` shows
  its own answer and its note only when the cursor reaches them.
