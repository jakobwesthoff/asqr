# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Questions and options take `context`: material shown next to the
  question that helps to answer it. An entry is an image, a text (a
  file or inline, such as source code, a log or Markdown) or a diff,
  which asqr computes from the old and the new text. A question or
  option holds one entry. This version shows text and diff entries as a
  placeholder that names them.
- `asqr validate` and `asqr ask` warn about a relative or missing file
  in any context entry, and about an `image` field left over from format
  version 1, saying that an image is a context entry of type `image`.
- `asqr validate` and `asqr ask` warn about a text or diff entry above
  1 MiB. asqr still shows it.
- A note to the whole session: the review tab has a field for it below
  Submit, and `enter` in it submits. A submitted result carries it as a
  top-level `note`, the first new field of result format version 2. A
  rejected result never has one. The draft keeps the note, and a draft
  with a note counts as started, so `asqr ask` refuses to replace it.
- The agent skill tells agents to read the session note.
- An example session with a diff and text entries in
  `examples/sessions/retry-review.json`.
- `i` shows the session's whole intro in a frame of its own, which
  scrolls with `↑`/`↓`, `j`/`k`, `pgup`/`pgdn`, `ctrl-u`/`ctrl-d`,
  `g`/`G` and `home`/`end` when the intro is longer than the screen.
  `esc`, `q` and `i` close it.
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
- `asqr new` writes format version 2 with a context entry.
- The agent skill describes format version 2 and context entries.
- A long own answer and a long reject reason wrap onto further lines
  under their start, like a note, instead of scrolling sideways while
  typed and being cut with `…` otherwise. The counter of a focused own
  answer moves to a line of its own after the text. The answer itself
  stays one line.
- The question and the review keep to a reading width of 100 columns
  on wide terminals. Beside the question, context takes the rest of the
  width from a terminal of 103 columns on; below it, context takes at
  least 8 rows and the question at most half the height. Where neither
  fits, context shrinks to a line that names it, and `z` still shows an
  image full screen.
- Context shows in a pane whose first line names the entry by its
  title or file name and, when the entry is taller than the pane, says
  which of its lines show. Beside or below the question, an image gets
  one row less for that line.
- `z` shows any context entry full screen, not only an image, and the
  one-line hint on small terminals offers it for every entry.
- The session title and intro wrap at the reading width instead of
  running to the edge. The title takes at most two lines, one on
  terminals under 24 rows. A cut intro ends in `… i: more`.
- The agent skill says that a cut intro can be read with `i`.
- The agent skill explains that the intro shows in at most three lines
  and only identifies the session, and that everything needed to answer
  belongs in the question text.

### Removed

- **Breaking:** the `image` field of questions and options. An image is
  a context entry: `"context": [{"type": "image", "path": ...}]`.
- The JSON Schemas of format version 1, `schema/session.v1.json` and
  `schema/result.v1.json`. `asqr schema` prints the version 2 schemas,
  which ship as `schema/session.v2.json` and `schema/result.v2.json`.

### Fixed

- `asqr ask` no longer adds `"options": null` to a `text` question in
  the copy it drops into the inbox.
- The text cursor of a note or a `text` answer stays visible when the
  text fills its line exactly or the cursor sits on the space where a
  line breaks. Before, the cursor's cell fell past the end of the line
  and could not be seen.
- A word that changes its style midway, such as the word under the text
  cursor or a word with Markdown emphasis in only part of it, wraps as
  one word. Before, it could be split at the style change. A word that
  ends exactly at the end of a line also stays on that line.

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
