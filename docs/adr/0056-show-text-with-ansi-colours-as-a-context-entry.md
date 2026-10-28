# 56. Show text with ANSI colours as a context entry

Date: 2026-10-22

## Status

Accepted

Amends [9. Crate choices](0009-crate-choices.md):
anstyle-parse reads the escape sequences of an `ansi` entry

Amends [15. Configure through flags and environment and log into the cache directory](0015-configure-through-flags-and-environment-and-log-into-the-cache-directory.md):
an `ansi` entry drops its own colours under `NO_COLOR`, RGB ones included

Amends [28. Session and result format version 2](0028-session-and-result-format-version-2.md):
a fifth context entry type, `ansi`

Amends [31. Make the context area a pane with a header, focus and scrolling](0031-make-the-context-area-a-pane-with-a-header-focus-and-scrolling.md):
the header names an `ansi` entry without a title or a path by its kind

Amended by [75. Cut a text entry after 500,000 lines](0075-cut-a-text-entry-after-500000-lines.md)

## Context

- Format version 2 had four context entry types: `image`, `code`,
  `markdown` and `diff` (ADR 28). None showed text in colours the asker
  chooses: asqr highlights a `code` entry itself, by its language or
  file extension.
- A mockup of a terminal screen could only be shown as an image, drawn
  in the colours of one terminal theme, so it matched only that theme.
  asqr scales an image to the pane, so its text was small until `z`
  showed it full screen. Coloured command output, such as
  `git diff --color` or a test run, had no way into the pane with its
  colours.
- New context entry types are added within format version 2 (ADR 51).
- Under `NO_COLOR` the pass after drawing resets named and indexed
  colours and keeps RGB colours, which images draw with (ADR 15).
- anstyle-parse 1.0 is in asqr's build already, through clap, with its
  default features.

## Decision

- **The type:** `ansi`, with an optional `title` and exactly one of
  `path` and `content`, as `code` and `markdown`. Its text carries SGR
  escape sequences (`ESC [ … m`).
- **Cell by cell:** asqr draws the text in the colours and attributes
  its SGR sequences set. The named colours, `30` to `37` and `90` to
  `97` and their backgrounds, are the slots 0 to 15 of the terminal's
  palette, so they follow the person's theme. `38` and `48` take `5;n`
  for a slot of 256 and `2;r;g;b` for an RGB colour, with `;` or `:`.
- **Attributes:** bold, dim, italic, underlined (`21` and the underline
  styles `4:1` to `4:5` as the single underline ratatui has), reversed
  and crossed out, as the adapter already gives code and Markdown.
  Blink, hidden, fonts, overline and the underline colour `58` are
  skipped, `58` with its arguments, and so is a colour out of range or
  cut short.
- **Every other escape sequence is dropped**, so an entry cannot change
  the screen outside its text: cursor movement, erasing, titles,
  hyperlinks, every OSC, DCS, SOS, PM and APC string with its contents,
  and SGR sequences with a private marker or an intermediate byte, such
  as `ESC [ > 4 ; 2 m`. A dropped sequence leaves no mark in the pane
  and gets no warning; the skill says that only SGR shows, so the asker
  knows.
- **Parsing:** anstyle-parse runs the DEC parser's state machine and
  ends each sequence where a terminal does. It gets the text character
  by character, ASCII as itself and any other character as DEL, which
  prints between sequences and is ignored inside one; the reader keeps
  the character DEL stands for. The parser decodes UTF-8 only between
  sequences, and a byte of a character such as `✓` would otherwise end
  a DCS or APC string early and show the rest of it.
- **Sequences and lines:** a style holds across line ends, as in a
  terminal. A control character inside an escape or CSI sequence acts as
  outside it, so a line end there ends the line and the sequence goes
  on. A string runs to its terminator, as in a terminal. A sequence cut
  short at the end of the text is dropped, and so is an SGR sequence of
  more than 32 numbers.
- **Graphemes:** a character that continues the grapheme before it,
  such as an accent or a skin tone, stays in that grapheme's run of
  text when an SGR sequence comes between, so it is measured and drawn
  as one.
- **Lines of source:** the lines go through `SourceLine` (ADR 33): they
  do not wrap, the pane scrolls sideways under the `‹` and `›` markers,
  tabs expand to stops of 4 as in code entries, and control characters
  show as their pictures, backspace included. A `\r` at the end of a
  line does not show. There is no gutter with line numbers.
- **Files** are read as for code entries (ADR 33, ADR 34): an absolute
  path, a regular file, at most 8 MiB, no binary file, and a card when
  the file cannot be read. A text without a line shows a dim "empty",
  and a last line that holds nothing but escape sequences is no line.
- **`NO_COLOR`:** an `ansi` entry shows without any colour, RGB colours
  included, and keeps its attributes. The pass after drawing keeps RGB
  colours for images, so the viewer leaves the colours out when it is
  built. It reads the setting off the quote style the screen asks for,
  which `quote_style` chooses by `NO_COLOR`.
- **Size:** `asqr validate` and `asqr ask` warn about an `ansi` entry
  above 1 MiB, as for code, Markdown and diffs, counted with its escape
  sequences.
- **Header:** the pane names the entry by its `title`, the file name of
  its `path`, or its kind, `ansi`.
- **The crate:** anstyle-parse 1.0.0, at the version in the lock file,
  as a direct dependency.

## Consequences

- An asqr older than this type refuses a session with an `ansi` entry
  (ADR 51).
- With colours on, a screen shows RGB colours outside an image where an
  `ansi` entry sets them. ADR 15's screen without an image and without
  an RGB colour is one without an `ansi` entry, or one under `NO_COLOR`.
- anstyle-parse is a direct dependency and adds no crate to the build.
- The 8 MiB read limit bounds the bytes read, not the memory of what the
  viewer keeps.
