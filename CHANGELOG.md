# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
