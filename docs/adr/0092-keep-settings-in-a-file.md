# 92. Keep settings in a file

Date: 2026-10-31

## Status

Accepted

Amends [15. Configure through flags and environment and log into the cache directory](0015-configure-through-flags-and-environment-and-log-into-the-cache-directory.md):
settings may also come from a file

Amends [30. Cap the question at a reading width and place context by the terminal's shape](0030-cap-the-question-at-a-reading-width-and-place-context-by-the-terminals-shape.md):
the reading width is a setting

## Context

- ADR 15 has settings come from flags and environment variables only,
  with no config file.
- The reading width is fixed at 100 columns (ADR 30), a diff opens
  unified and `s` switches the diff shown without keeping the switch
  (spec 8.5), and the placement of context is a flag or a variable given
  at every start.

## Decision

- asqr reads a TOML file, `settings.toml`, from the platform's config
  directory, through the `directories` crate asqr already uses for its
  data and cache directories: `~/Library/Application Support/asqr/` on
  macOS, beside the queue data, and `$XDG_CONFIG_HOME/asqr/` on Linux.
  Only the TUI reads it.
- Three settings, each a key of the file with a fixed set of values:
  - `diff_layout`: `unified` (default) or `side-by-side`, how every diff
    opens.
  - `max_question_width`: 80, 100 (default), 120 or `"full"`, the widest
    a question's text gets.
  - `context`: `auto` (default), `beside` or `below`, where the context
    goes, as `--layout` and `ASQR_LAYOUT` say.
- A flag wins over an environment variable, an environment variable over
  the file, and the file over the default. `--layout` and `ASQR_LAYOUT`
  keep winning over `context`. The other two settings get no flag and no
  variable.
- The reading width is the `max_question_width` setting. It caps the
  question, the review and the header's text, and never moves the choice
  between context beside and below, which keeps going by the terminal's
  width and shape (ADR 30). The intro overlay and the dialog of an
  invalid session stay at 100 columns of text.
- `s` keeps switching the diff shown only. The setting decides how every
  diff opens, and a switch made with `s` never becomes the setting. A
  switched diff keeps the layout it was switched to when the setting
  changes.
- A file that cannot be read or does not parse gives every default. A
  value that is none of a setting's gives that setting's default and
  leaves the others as the file has them. Keys asqr does not know are
  ignored. Each case is a warning in the log and in the status line when
  the TUI starts, and never stops a command.
- Rejected: environment variables and flags only; `~/.config/asqr` on
  macOS too, and reading `~/.config/asqr` first with the Apple path
  after; JSON instead of TOML; any width from 60 to the terminal's width;
  letting the width move the choice between beside and below; switching
  the diff layout for the rest of the run, or saving it on `s`.
