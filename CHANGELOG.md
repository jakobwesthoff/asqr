# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
