# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The empty queue shows Talqi, asqr's mascot, above its sentence:
  awake from the start until the first session arrives, asleep once the
  queue is empty again. Terminals that show images draw the picture;
  the others, and those that report a cell size of zero pixels on a
  side, draw it in half blocks of its own colours. Talqi needs a
  terminal of at least 18 rows; the pictures are built into asqr. Under
  `NO_COLOR`, Talqi is the outline of asqr's emblem in braille
  characters, without any colour and the same awake and asleep, on
  terminals of at least 32 rows.
- The terminal title shows how many sessions wait, as
  `asqr · 2 waiting`. On quit asqr clears its title and gives the
  terminal its title from before back where the terminal keeps a title
  stack. `--no-title` leaves the title alone.
- asqr honours `NO_COLOR`: set to any value but the empty one, the TUI
  draws without colours and keeps bold, dim, reversed, the marks and
  the pointers. Images keep their colours. Before, the terminal library
  took the colours itself, with them the images, and text lost its dim
  or reversed style where its colour changed.
- The status line confirms a submitted or rejected session, as
  `✔ sent "Release 0.2.0" · 1 answered, 2 skipped` in green or
  `✗ rejected "Release 0.2.0"`, with the id for a session without a
  title. The line stays until the next key or paste, also over the next
  session and over the empty queue.

### Changed

- The TUI takes its colours from the roles of the design guide instead
  of reversing nearly everything. The row under the cursor, a field's
  head under it, the current entry of the session list and the focused
  context pane's header show a yellow fill, one at a time: while the
  context pane has the focus, the question's current row keeps only its
  pointer. The title line and the key bar lose their fills: `asqr` and
  the keys are bold cyan, the queue and the key descriptions dim. The
  current tab is bold cyan over a heavy cyan line, unanswered `☐` marks
  are dim, chosen option marks bold cyan and option numbers dim.
  `[ Submit ]` is always filled in cyan, with a yellow pointer when the
  cursor is on it. The help, the intro and the session list have rounded
  cyan frames with a blank column inside, which moves the help's keys
  and the session list's rows one column in.
- The status line colours each notice and message by its kind instead
  of showing all of them in yellow. Failures, such as a draft that
  cannot be saved or an inbox that cannot be read, are red. Refused keys
  and pastes and sessions set aside, such as an invalid session answered
  with an error, stay yellow. News, such as a session file reloaded
  after it changed, is in the text colour.
- The placeholder of an image that cannot be shown wraps and scrolls
  like the placeholders of code, Markdown and diffs. A long reason in a
  short pane was cut off before; now the pane's header says which lines
  show, and the pane scrolls to the rest.

### Fixed

- The notes around a Markdown file cut at 8 MiB are cut at the pane's
  edge, as those of code and diffs are. Before, they wrapped onto
  further rows in a pane narrower than the note.

## [0.11.0] - 2026-10-15

### Added

- Questions and options take `context`: material shown next to the
  question that helps to answer it. An entry is an image, code (text
  shown as written and highlighted, such as source, a log or a
  configuration file), Markdown (rendered) or a diff, which asqr
  computes from the old and the new text. Code and Markdown come from a
  file or inline in `content`. A question or option holds one entry.
  `"context": null` counts as no context, as `null` does for every
  other optional field.
  asqr reads only regular files, so a directory or a FIFO shows a
  placeholder instead of stalling the screen, and of a text file at
  most its first 8 MiB, under a dim "first 8 MiB shown" and above a dim
  "… the rest is cut after 8 MiB".
- A code entry shows highlighted in its language, through the Markdown
  renderer patine and the 8 colours of the terminal's palette, with line
  numbers in a dim gutter. The language comes from `language`,
  else from the extension or file name of `path` or `title`. Long lines
  do not wrap; the pane scrolls sideways, a `›` marks a line cut at the
  right edge and a `‹` a line scrolled past the start of its text.
- A Markdown entry shows rendered in patine's style and wrapped to the
  pane: headings, emphasis, inline code, lists, quotes, tables and
  highlighted code blocks. A line patine leaves wider than the pane,
  such as a long line of code, wraps, and each further row starts with a
  dim `↪`, indented like the line's quote bars, code block or list item
  text.
- A diff entry shows as a unified diff: hunks with 3 lines of context, a
  separator between hunks, the old and the new line number in a dim
  gutter, removed lines red and added lines green, and the changed words
  of a changed line marked. A diff without old text is a new file, one
  without new text a deleted one. Long lines do not wrap; the pane
  scrolls sideways, and `›` and `‹` mark cut lines as in a code entry.
  A wide character that would reach under a marker shows as blanks, so
  the marker stays visible. Lines compare without their line ends, and
  a dim "line endings differ" says when the two sides use different
  ones, such as `\r\n` and `\n`. Tabs expand to stops of 4 columns, and
  other control characters show as their pictures, such as `␛`. The
  agent skill describes what to put into `old`, `new`, `name` and
  `title`.
- An image entry takes `"enlarge": false` to keep the image at its
  natural size where it fits, in the pane and full screen, instead of
  scaling it up to fill the space. A larger image still shrinks. The
  agent skill tells agents to use it only where the exact pixel size
  matters.
- `asqr validate` and `asqr ask` warn about a relative or missing file
  in any context entry, and about an `image` field left over from format
  version 1, saying that an image is a context entry of type `image`.
  An entry of type `text` is an error that names `code` and `markdown`.
  A diff with neither `old` nor `new`, or with `path` or `content` on
  the entry itself, is an error that says what a diff takes: the text
  before and after the change, which asqr diffs itself.
- `asqr validate` and `asqr ask` warn about a code, Markdown or diff
  entry above 1 MiB. asqr still shows it.
- `asqr validate` and `asqr ask` warn when a side of a diff looks like a
  unified diff, which asqr would diff like any other text, and say to
  give the text before and after the change as `old` and `new`.
- `asqr validate` and `asqr ask` warn "not a file" about a context path
  that names a directory or another file that is not a regular file.
- The JSON Schemas state the format version as the constant `2`, allow
  one context entry per question or option, require exactly one of
  `path` and `content` in a code or Markdown entry and in each side of a
  diff, and require `old`, `new` or both in a diff. They describe
  `intro`, question `text` and option `description` as Markdown in
  which a single newline breaks the line and raw HTML shows as written.
- A note to the whole session: the review tab has a field for it below
  Submit, and `enter` in it submits. A submitted result carries it as a
  top-level `note`, the first new field of result format version 2. A
  rejected result never has one. The draft keeps the note, and a draft
  with a note counts as started, so `asqr ask` refuses to replace it.
- The agent skill tells agents to read the session note.
- An example session with diff, Markdown and code entries in
  `examples/sessions/retry-review.json`.
- `i` shows the session's whole intro in a frame of its own, which
  scrolls with `↑`/`↓`, `j`/`k`, `pgup`/`pgdn`, `ctrl-u`/`ctrl-d`,
  `g`/`G` and `home`/`end` when the intro is longer than the screen.
  `esc`, `q` and `i` close it.
- `tab` moves into the context pane, which then scrolls with `↑`/`↓`,
  `j`/`k`, `←`/`→`, `h`/`l`, `g`/`G`, `home`/`end`, `pgup`/`pgdn` and
  `ctrl-u`/`ctrl-d`; `tab`, `esc` or `q` move back to the question or
  the field being typed in, so a word typed after `tab` never quits.
  `pgup`/`pgdn` and `ctrl-u`/`ctrl-d` scroll the pane from anywhere,
  also while typing in a field. Each entry keeps its
  scroll position while the person moves between options and
  questions. The key bar and the help show the pane's keys while it has
  the focus.
- Pasting into a field or a note inserts the text as a whole, through
  the terminal's bracketed paste. A line break in it no longer acts as
  `enter`, which picked an option, submitted the session from its note
  or rejected it. The pasted line breaks stay in the text, and `\r\n`
  or a lone `\r` arrive as `\n`. A tab becomes blanks up to the next
  multiple of 4 columns and other control characters are dropped, in a
  paste and in text restored from a draft, as a field shows neither. A
  paste on a field left with `esc` gives it the keys again and goes in;
  elsewhere a message says that a paste goes into a field. asqr turns
  bracketed paste off again when it quits or panics. A terminal that
  refuses bracketed paste, as a legacy Windows console does, runs asqr
  without it.
- `--layout auto|beside|below` and `ASQR_LAYOUT` choose where a
  question's context goes. `auto`, the default, puts it beside the
  question when the terminal's area looks landscape and below it when
  it looks portrait, and keeps what it shows while a resize stays near
  a square shape.

### Changed

- **Breaking:** session and result files use format version 2
  (`"asqr": 2`). asqr refuses a version 1 session with a message that
  points to `asqr skill`, which describes the new format. A version 1
  session still waiting in a queue gets an error result with that
  message. `wait` and `result` still print a version 1 result that is
  already in the outbox.
- `asqr ask` makes every path in a context entry absolute, as it did
  for images.
- `asqr new` writes format version 2 with a context entry, and without
  an id, which `asqr ask` assigns, as the agent skill says.
- An `asqr` version that is no unsigned integer, such as `2.0` or
  `"2"`, is refused as an unsupported format version with the value as
  written, instead of a parser error about its type.
- The agent skill describes format version 2 and context entries.
- A long own answer and a long reject reason wrap onto further lines
  under their start, like a note, instead of scrolling sideways while
  typed and being cut with `…` otherwise. The counter of a focused own
  answer moves to a line of its own after the text.
- The question and the review keep to a reading width of 100 columns
  on wide terminals. Beside the question, context takes the rest of the
  width from a terminal of 103 columns on; below it, context takes at
  least 8 rows and the question at most half the height. Below, the
  question takes the rows the tallest question with context in the
  session needs, so the context stays in place between questions, and
  typing moves it only when that question gains a line. Where neither
  fits, context shrinks to a line that names it, and `z` still shows an
  image full screen. A dim `↑ more` or `↓ more` on the first or last row
  of the question and the review says that rows are cut off there; it
  never covers the text cursor.
- Context shows in a pane whose first line names the entry by its
  title or file name and, when the entry is taller than the pane, says
  which of its lines show. While the pane does not have the focus and
  its entry is taller or wider than the pane, the header also names
  `tab`, as in `1–10 of 31 · tab`. Beside or below the question, an
  image gets one row less for that line, and an image smaller than the
  pane grows to fill it.
- `z` shows any context entry full screen, not only an image, and the
  one-line hint on small terminals offers it for every entry, as
  `esc, z: full screen` while a field or a note has the keys. The
  full-screen view takes every key, as the focused pane does: it
  scrolls with the pane's keys, the question's keys no longer act
  behind it, and `z`, `esc`, `tab` or `q` go back to where the focus
  was. Its key bar shows the pane's keys, and its help lists them with
  `z/tab/esc/q` to leave the full screen. A resize that passes through
  a size too small for asqr keeps the full-screen view and the pane's
  focus.
- `o` on a code, Markdown or diff entry, or where no entry shows, says
  why it opens nothing.
- `tab` and `shift-tab` no longer type a tab character into a field.
  While a context pane shows, `ctrl-u` and `ctrl-d` scroll it instead of
  undoing and deleting in a field.
- Every typed field takes several lines: the own answer, the session
  note and the reject reason now accept `ctrl-j` like a `text` answer
  and a question's note, and `enter` still picks, submits or rejects.
  In results, `custom`, the session `note` and `reason` may contain
  `\n`; the format stays version 2. The review shows a line
  break in an own answer as `⏎`. The key bars of these fields show
  `ctrl-j new line`, and the agent skill says that these fields may
  span several lines.
- `↑`/`↓` in a typed field move the text cursor through the rows the
  field shows, wrapped rows included, and leave the field only from its
  first or last row; `ctrl-p`/`ctrl-n` do the same. Moving down onto a
  field puts the text cursor at the end of its first row, moving up
  onto it at the end of its text. Before, they left the own answer, the
  `text` answer, the session note and the reject reason at once, so the
  cursor could not reach an earlier line of a `text` answer, and in a
  question's note they moved by the text's lines. The key bars of these
  fields drop their `↑/↓ leave` and `↑ back` hints, and the help states
  the rule.
- The key bar leaves out hints that do not fit whole instead of cutting
  the last one, and keeps `? help` and `q quit` longest.
- The help puts `space` and `1`-`9` on one line and `n` and `ctrl-j` on
  another, to make room for the context pane's keys.
- The agent skill explains that context shows in a pane of its own that
  the person scrolls and can show full screen, so long material belongs
  there rather than in the question text.
- The session title and intro wrap at the reading width instead of
  running to the edge. The title takes at most two lines, one on
  terminals under 24 rows. A cut intro ends in `… i: more`, or in
  `… esc, i: more` while a field, a note or the context pane has the
  keys.
- The agent skill says that a cut intro can be read with `i`.
- The agent skill explains that the intro shows in at most three lines
  and only identifies the session, and that everything needed to answer
  belongs in the question text.
- The intro, question texts and option descriptions render full
  Markdown in patine's style, as a Markdown entry does: headings,
  emphasis, inline code in yellow instead of reversed, lists, quotes,
  links, tables, rules and highlighted code blocks. Before, everything
  but bold, italic, inline code, lists and line breaks showed as its
  source. A line wider than its space, such as a long line of code,
  wraps, and each further row starts with a dim `↪`. A single newline
  still breaks the line, and raw HTML, such as `Vec<String>`, still
  shows as written.
- The agent skill says that the intro, question texts and option
  descriptions take full Markdown, that a single newline breaks the line
  and HTML shows as written there, that option descriptions should stay
  short, and that long code belongs in a `code` entry.
- The agent skill says that a Markdown entry follows CommonMark instead:
  a single newline joins the lines, a blank line starts a paragraph, a
  trailing backslash breaks the line, and raw HTML is left out, so `<`
  goes into code spans.
- A session file of the wrong shape names the field in its error, as
  in `not a session file: questions[1].context: invalid type: map,
  expected a sequence`. Before, the message named no question and no
  field. A `context` that is no list adds `context is a list with one
  entry: wrap the entry in [ ]`.

### Removed

- **Breaking:** the `image` field of questions and options. An image is
  a context entry: `"context": [{"type": "image", "path": ...}]`.
- The JSON Schemas of format version 1, `schema/session.v1.json` and
  `schema/result.v1.json`. `asqr schema` prints the version 2 schemas,
  which ship as `schema/session.v2.json` and `schema/result.v2.json`.

### Fixed

- When the help, the intro or the session list closes while an image
  shows, asqr clears the terminal and draws the whole screen again.
  With Sixel and the iTerm2 protocol, ratatui never rewrites the cells
  an image covers, so text of the overlay could stay on the image.
- The warnings of `asqr ask` and `asqr validate` name a missing context
  file by its absolute path when the session file was given by its bare
  name. Before, they printed the path as the session file wrote it.
- A session file replaced while asqr shows it keeps the question or
  review shown, the row, a field's text cursor, whether the field had
  the keys, an open note, the context pane's focus, the full-screen view
  and the scroll positions of the entries that did not change, and a
  message says that the file was reloaded. Before, the session started
  over at its first question without a word. An open intro overlay
  closes when the new file has no intro, where it stayed open with
  nothing in it and took every key.
- Removing a session while the session list was open on the last one
  left the cursor past the end, and `enter` showed "Nothing to answer"
  while sessions waited. The cursor stays on a session, and the list
  closes when none is left.
- `asqr ask` no longer adds `"options": null` to a `text` question in
  the copy it drops into the inbox.
- `asqr validate` and `asqr ask` refuse a session file that is no JSON
  object as not a session file (exit code 11). Before, a session written
  as an array of its fields in their order passed validation, and
  `asqr ask` crashed on it.
- The text cursor of a note or a `text` answer stays visible when the
  text fills its line exactly or the cursor sits on the space where a
  line breaks. Before, the cursor's cell fell past the end of the line
  and could not be seen.
- A word that changes its style midway, such as the word under the text
  cursor or a word with Markdown emphasis in only part of it, wraps as
  one word. Before, it could be split at the style change. A word that
  ends exactly at the end of a line also stays on that line.
- Text with wide characters, such as Chinese, Japanese or Korean or
  most emoji, keeps to the screen's width. Each such character takes two
  columns, but asqr counted it as one, so a wrapped own answer, note,
  session note, reject reason or `text` answer ran past the edge, and a
  cut title or intro lost its `…` or `… i: more`. The same held for the
  tab bar, the review's labels and answers, the description column of
  options, the context pane's header and the queue path in the title
  bar. A wide character that does not fit at the end of a line now
  starts the next one. The text cursor shows on the whole character it
  sits on, an emoji with its skin tone included. In the context pane, a
  halfwidth sound mark such as `ﾞ` takes a column of its own, as
  terminals draw it.
- The `—` of an empty `text` answer and an empty note is dim, as it is
  in an empty own answer. Before, it showed in full brightness.
- The session schema requires at least one question, as validation
  does. Before, it accepted `"questions": []`, which `asqr validate`
  refuses.
- When asqr panics, it turns the terminal's focus reports off before
  it ends. Before, they stayed on, and the shell received them as typed
  text whenever its window gained or lost the focus. A signal that ends
  asqr, such as SIGTERM, still leaves them on. A terminal that refuses
  focus reports runs asqr without them. Before, asqr ended with an error
  and left the terminal in raw mode on its alternate screen.
- Run over ssh from a tmux pane without passthrough, asqr no longer
  asks the terminal for its graphics protocol and shows images as
  blocks. The question got no answer there, and the thread waiting for
  it took the first input that came. A first paste then lost its start
  marker, and its line breaks arrived as `enter`, which could pick an
  option or submit the session. Where `TERM` starts with `tmux`, or
  `TERM_PROGRAM` is `tmux`, without `TMUX`, asqr first sends a status
  request through tmux's passthrough and asks for the graphics protocol
  only when the answer comes within 300 ms, so tmux with passthrough on
  still shows images inline.
- Inside tmux, asqr gives its pane's `allow-passthrough` setting back
  once it has detected the graphics protocol, and unsets it when the
  pane had no setting of its own. Before, ratatui-image set it to `on`
  on every start, and it stayed on after asqr ended.
- An own answer of nothing but blanks or line breaks no longer shows
  as chosen. The tab bar, the review and the result already treated it
  as no answer.
- In a field, `pgup`/`pgdn`, `ctrl-v` and `alt-v` do nothing without a
  context pane. Before, they moved the text cursor to the start of the
  text.
- `shift` with a move in a field only moves the text cursor. Before, it
  selected text that asqr did not show, and the next typed key, paste
  or delete replaced it.
- A field or a note taller than the question's area keeps its text
  cursor in view, and on the text's last line the length counter below
  it too. Before, the question showed the field's start, and the cursor
  of a long `text` answer could stay below the screen while the person
  typed.

## [0.10.1] - 2026-10-14

### Added

- Every GitHub release carries prebuilt binaries for macOS (Apple
  silicon and Intel) and Linux (x86_64 and aarch64, statically linked).

## [0.10.0] - 2026-10-06

### Added

- `asqr status --since <age>` lists only sessions answered within that
  age, such as `2h`. `--limit <count>` sets how many answered sessions
  show, and `--all` lists every one.
- `asqr status --json` reports `answered_total`, the number of answered
  sessions before the limit cuts the list.

### Changed

- `asqr status` lists the 10 most recently answered sessions, newest
  first, instead of every result in the outbox in id order. The outbox
  keeps results until they are pruned, so the full list kept growing
  and cost an agent tokens on every call. Waiting sessions are still
  all listed.
- The agent skill explains the limit, `answered_total` and how to find
  a lost session with `--since`.

## [0.9.2] - 2026-10-03

### Fixed

- An option whose `description` is empty or only spaces shows again.
  Before, its whole row disappeared in wide terminals, and the person
  could not pick it.
- A paragraph after a quote, heading, code block, HTML block or
  horizontal rule starts on its own line after a blank line. Before, it
  was glued onto the last line of that element.
- Images in terminals with the Kitty protocol are no longer cropped or
  drawn too small when the cell size differs from the one at start:
  after a font size change, or in tmux with clients of different cell
  sizes attached.

### Changed

- The agent skill says that Markdown outside the supported subset, such
  as quotes, headings, links and code blocks, shows as literal source,
  and how to set a line apart instead.
- The log records the detected graphics protocol and cell size at start,
  and the terminal's size on every resize.

## [0.9.1] - 2026-09-29

### Added

- `asqr validate` and `asqr ask` warn about an image asqr cannot show,
  such as SVG or AVIF, and list the formats that work. Before, the
  person saw a placeholder and the asker never heard about it.
- The agent skill and the `image` field of the session schema name the
  supported image formats: PNG, JPEG, GIF (first frame only), WebP,
  BMP, TIFF, ICO, TGA, PNM, QOI, DDS, OpenEXR, HDR and farbfeld.

## [0.9.0] - 2026-09-28

The first release.

### Added

- A terminal UI (`asqr`, or `asqr watch`) that shows the sessions
  waiting in a queue and lets you answer them with the keyboard: a tab
  per question, a review tab to submit or reject the whole session, and
  a draft that keeps your answers when you quit.
- Question kinds: single choice, multiple choice with optional `min`
  and `max`, and typed answers. Choice questions can take your own
  answer instead of the given options, and a question takes a note
  unless the asker turns that off. Every question is optional.
- Option descriptions, preselected defaults, and length hints with a
  counter for typed answers. The intro, question texts and option
  descriptions support bold, italic, inline code, lists and line
  breaks.
- Images next to a question, or one per option that shows while the
  cursor is on it, drawn inline in terminals with the Kitty, iTerm2 or
  Sixel protocol and as blocks elsewhere. `o` opens an image in the
  system viewer, `z` shows it full screen.
- A desktop notification and the terminal bell when a session arrives
  (`--no-notify`, `--no-bell`), passed through tmux when it allows it.
- Commands for askers: `new`, `validate`, `ask`, `wait`, `result`,
  `status`, `prune`, `paths` and `schema`. `wait` and `result` exit
  with 0 for a submitted session, 10 for a rejected one, 11 for an
  invalid file, 12 while no result is there yet and 13 when none will
  come.
- Named queues (`--queue`, `ASQR_QUEUE`) and queues in any directory
  (`--dir`).
- Version 1 of the session and result format, with JSON Schemas in
  `schema/`.
- An agent skill built into the binary: `asqr skill` prints it,
  `asqr skill --install <dir>` installs it.
