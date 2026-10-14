---
title: "Split the TUI into a question pane and a context pane"
kind: plan
component: tui
horizon: next
origin: request
tags: [ux, milestone/v0-11]
---
# Split the TUI into a question pane and a context pane

The user asked for this on 2026-10-14. The screen splits into two panes,
side by side or stacked depending on the terminal's size. The question
pane, left or on top, keeps today's interface at a width that is still
comfortable to read. The context pane, right or below, shows material
that helps answer a question: images as today, plus viewers for longer
text (source code with syntax highlighting, Markdown) and for diffs. The
pane scrolls and is navigated on its own, and more viewers can be added
later.

This plan is phase 1: one context entry per question or option, shipped
in 0.11.0 with format version 2. Several entries per question are phase
2, in their own todo. The design below was settled with the user in
four asqr rounds on 2026-10-14 (sessions `01M4XEHY7QSW6Y5JBNWAXBYV9Y`,
`01M4XFKQ2V7NK2NWKAP9QEEZB2`, `01M4XGMR0HK8KKZYHH9SF3NSTD`,
`01M4XHEY66BQDA0ZPRXV3P22MD`). Each step writes the ADR, spec, schema
and skill changes for its part.

## Steps

The format step, session and result files in format version 2 with a
`context` field, has landed (ADR 28), and so have the layout step, which
caps the question at a reading width and places the context area
beside or below it (ADR 30), the pane step, which made that area a
pane with a header, viewers, focus and scrolling (ADR 31), and the diff
step, which shows diff entries as a unified diff (ADR 33). "Show code
and Markdown entries in the context pane" comes next. Its first part
has landed: `code` and `markdown` replace the `text` entry type in
format version 2 (user, 2026-10-14; ADR 28, amendment). It then
implements the pane's `Viewer` trait
(`src/tui/render/viewer.rs`) for both in place of the placeholder viewer
there. The code viewer draws source lines through
`src/tui/render/source_line.rs` as the diff viewer does; both use patine
1.5.0, published 2026-10-14.

Full Markdown in question texts ships in the same release as its own
todo; the wrapped own answer (ADR 27) and the session note on the review
tab (ADR 29) have landed already. The Markdown todo and the Markdown
viewer share the adapter from patine's lines to ratatui styles;
whichever lands first builds it.

## Where asqr stands

- `src/tui/render/screen.rs`, `draw`: below 60×15 asqr only says what
  size it needs (`MIN_WIDTH`, `MIN_HEIGHT`). Above that, the screen is a
  vertical stack: title, header (session title and intro), tab bar, the
  tab's content, status line, key bar. The question, the review and the
  header text keep to a reading width of 100 columns (ADR 30).
- `header_lines`: the title wraps onto at most two lines and the intro
  onto three (one each below 24 rows); a cut intro ends in `… i: more`,
  and `i` shows all of it in an overlay that scrolls.
- `draw_tab`: once a question or any of its options has context
  (`has_context_area`), the question keeps room for it while moving
  through the options. `src/tui/render/panes.rs` places the context
  area beside, below, or as a one-line hint by the rules under
  *Layout*, with `--layout` and `ASQR_LAYOUT` (ADR 30).
- Context attaches to a question (`context`) and to an option
  (`context`), one entry each; the option under the cursor shows its
  own, otherwise the question's (`shown_entry`). The context pane
  (ADR 31, `src/tui/render/context.rs`) shows a header that names the
  entry and the entry's viewer below it. Image entries show as images,
  diff entries as a unified diff (ADR 33, `src/tui/render/diff.rs`),
  and code and Markdown entries as a placeholder viewer that names them. `tab` focuses the pane,
  the page keys scroll it from anywhere, and each entry keeps its scroll
  position. `o` opens the shown image in the system viewer and ignores
  the other entries; `z` shows any entry
  full screen. The review tab shows no context.
- Session and result files are format version 2 (ADR 28, schemas in
  `schema/`). An invalid session file gets a result with status `error`
  and is archived (spec section 3.8).
- Keys: `ctrl-c` quits and `ctrl-j` adds a line in fields; `tab`,
  `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` belong to the context pane, and
  the focused pane takes every key (`pane_key` in `src/tui/session.rs`).
  Every other `ctrl` combination is free outside fields (`command`
  ignores them). Also free on the question: `shift-tab`, `home`, `end`,
  `J`, `K`, `g`, `G`, `/`. In a field, every printable key types text,
  so keys that must work there are non-printing ones.

## Design

### Layout

W×H is the area under the tab bar; cells are about twice as tall as
wide, so the visible shape is W ÷ 2H.

- The context pane exists per question: on a question that has context
  or has an option with context. The pane keeps its room while the
  cursor moves through the options, as images do today.
- The question text always has a reading width cap of about 100 to 110
  columns (to be tuned by trying it), with or without a pane. Without a
  pane it is left-aligned.
- Beside: the question pane takes half the width until it reaches the
  cap; the context pane gets the rest after a one-column separator.
  Beside is possible from 101 columns (question at least 60, pane at
  least 40).
- Below: the question takes the rows it needs up to half the height, the
  pane the rest. Below is possible when the pane gets at least 8 rows.
  The question text keeps its cap; the pane uses the full width.
- `--layout auto` (the default) picks: when both fit, a landscape area
  (W ÷ 2H at least 1) goes beside and a portrait one below, with
  hysteresis on resize (switch to beside from 1.1, back to below under
  0.9). When only one fits, that one. When neither fits, the pane
  shrinks to a one-line hint and `z` shows the item. Examples: 120×40
  and 230×60 beside, 110×70 and 90×50 below.
- `--layout beside|below` (and an environment variable, ADR 0015 style)
  overrides the automatic choice.
- A question without context uses the width it can get, so on a terminal
  narrower than about twice the cap its text is wider than on a question
  with a pane, and reflows when moving between the two.
- Header: the session title and intro wrap at the reading width; the tab
  bar keeps the full width. The intro still shows at most three lines,
  the cut is marked `… i: more`, and `i` shows the whole intro in an
  overlay.
- The review tab shows no context.

### Format version 2

- Session and result files both move to version 2. asqr drops version 1
  entirely. A file with a lower version is refused with a message that
  this asqr no longer supports format version N and that `asqr skill`
  describes the current format. Version 1 sessions waiting in a queue
  when asqr is upgraded get such an error result (status `error`), as
  any invalid file does, so the waiting agent sees exit code 11 and the
  hint.
- Questions and options get a `context` field: a list, limited to one
  entry in phase 1, so phase 2 lifts the limit without a format change.
  The `image` field is folded into it and goes away.
- Every entry has a `type` and an optional `title` for the pane's
  header.
  - `image`: `path`, optional `enlarge` (ADR 32).
  - `code`: `path` or inline `content`, an optional `language`. Shown
    verbatim and highlighted, like a fenced code block. Without a
    language, the extension of `path` (or of `title`) decides; failing
    that, plain text.
  - `markdown`: `path` or inline `content`, rendered with patine.
  - `diff`: `old` and `new`, each `{ "path": ... }` or `{ "content": ... }`,
    an optional `name` (the file name in the header and the source of
    the language) and `language`. A missing `old` is a new file, a
    missing `new` a deleted one. asqr computes the diff; a hand-written
    unified diff goes wrong easily, as every hunk header needs exact
    line counts.
- `asqr ask` makes every path absolute, as for images today.
- `validate` and `ask` warn about an entry above 1 MiB (file or inline);
  asqr still shows it. Files are read when first shown.
- The result gains a session-level `note` (landed, ADR 29).

### Viewers

- Highlighting: `syntect` with `two-face`'s grammars and the 8-colour
  ANSI theme, like patine, so the terminal's theme decides the colours.
  Shared with patine through its library (patine todo
  01M4XHNXXNJNV57SNBYH88EFAN).
- Code: lines longer than the pane scroll sideways, with a dim `›`
  marking a cut line, and a dim gutter with line numbers on every code
  entry. A `.md` file as a code entry shows its source.
- Markdown entries render through the patine-style renderer, wrapped,
  without gutter or sideways scrolling.
- Diff: unified in phase 1, side by side later (its own todo, for panes
  of about 160 columns and more). Hunks with 3 context lines and a dim
  separator between hunks, old and new line numbers in a dim gutter,
  changed words emphasised inside changed lines. No syntax highlighting
  inside diffs in phase 1, and no whole-file view.

### Keys

- Always on the pane, even while typing in a field: `pgup`/`pgdn` page
  through it, and `ctrl-u`/`ctrl-d` too (laptops without page keys;
  layout-independent).
- `tab` gives the pane the focus, `tab` or `esc` gives it back. While it
  has the focus: `↑`/`↓` and `j`/`k` scroll by line, `←`/`→` and `h`/`l`
  sideways, `g`/`G` and `home`/`end` to the top and bottom.
- `z` and `o` act on the item shown, from the question as today and from
  the focused pane. `z` shows any item full screen; `o` opens images
  only (the other entry types follow in their own todo).
- No `[`/`]` or other keys that need AltGr or Option on non-US layouts.

### Agent skill

- The skill already tells agents what belongs in the intro and that
  everything needed for a decision goes into the question text (done
  2026-10-14, ahead of this work). This work adds `i`, format version 2
  with `context`, and the viewers.

## Decisions

- 2026-10-14, user: the pane exists per question; the question text is
  capped at a reading width of about 100 to 110 columns in any case,
  left-aligned without a pane; below the cap the panes split half and
  half.
- 2026-10-14, user: the automatic beside/below rule as proposed by the
  agent, overridable with `--layout`; stacked, the question takes the
  rows it needs up to half the height.
- 2026-10-14, agent (left to the agent by the user): a question without
  context reflows to the wider width it can get. Changing the question
  replaces the whole text anyway, and a session-wide width would make
  questions without context needlessly narrow on 120 to 200 columns.
- 2026-10-14, user: header text wraps at the reading width, `i` shows
  the full intro, the skill explains what the intro is for.
- 2026-10-14, user: context attaches to questions and options; several
  entries per question are phase 2; the review tab shows no context.
- 2026-10-14, user: format version 2 for sessions and results, version
  1 dropped with a message that points to `asqr skill`, waiting version
  1 sessions get an error result; the entry fields and the 1 MiB
  warning as listed above.
- 2026-10-14, agent (delegated), confirmed by the user: diffs are given
  as old and new text. An agent can always produce the pair, while a
  hand-written unified diff goes wrong easily as every hunk header needs
  exact line counts.
- 2026-10-14, user: the keys as listed above.
- 2026-10-14, user: ANSI-theme highlighting, sideways scrolling, line
  numbers on source views only, unified diffs with hunks, line numbers
  and word emphasis.
- 2026-10-14, user: the `text` entry type is replaced by `code`
  (verbatim, highlighted) and `markdown` (rendered), each with its own
  viewer, and the `render` flag goes away. The inline content key
  `text` becomes `content` in every entry, so "text" and "kind" each
  mean one thing in the format. Reasons in the step todo "Show code and
  Markdown entries in the context pane".
- 2026-10-14, user: patine's library renders to style-neutral lines that
  asqr shares (patine 1.5.0), as planned in patine todo
  01M4XHNXXNJNV57SNBYH88EFAN.
- 2026-10-14, user: 0.11.0 carries phase 1, the wrapped own answer, the
  session note on the review tab, and full Markdown in question texts.
- 2026-10-14, user (asqr session `01M4XJX6BJADVKKXNXXFNVVC8Q`): the
  steps that need patine 1.5.0 wait until the user has published it to
  crates.io, without a path or git dependency in between. The work ends
  with a release commit for 0.11.0 (version bump, dated CHANGELOG) but
  no tag, push or publish.

## Relations

- Relates to: [Full Markdown in question texts, rendered like patine](01M3MM92NYZQ4SG4D8R821M7X7-full-markdown-in-question-texts-rendered-like-patine.md), same renderer and highlighting
