# 9. Crate choices

Date: 2026-09-27

## Status

Accepted

Amended by [35. Render the text fields as full Markdown through patine](0035-render-the-text-fields-as-full-markdown-through-patine.md)

## Context

The tool uses clap, anyhow and/or thiserror, and other
best-practice crates where they are needed.

## Decision

| Need | Crate |
|---|---|
| Command line | `clap` with derive |
| Typed errors in the format and queue code | `thiserror` (validation errors name the field) |
| Errors at the binary's edges | `anyhow` with `.context()` |
| TUI | `ratatui`, `crossterm`, `tui-textarea`, `ratatui-image` |
| Watching | `notify` with `notify-debouncer-full` |
| Paths | `directories` |
| Format | `serde`, `serde_json`, `schemars` |
| Atomic writes | `tempfile` |
| Timestamps | `jiff` |
| Logging | `tracing` and `tracing-subscriber`, into a file in the platform cache directory |
| Markdown subset | `pulldown-cmark`, rendering only the allowed elements |
| Session ids | `ulid` |
| Tests | `insta`, `assert_cmd` |

Crates are added with `cargo add` when the first test needs them.

## Consequences

The tracing output goes to a file, because the TUI owns the terminal.

## Amendment (2026-09-27)

`ratatui-textarea` replaces `tui-textarea` for the text fields.
`tui-textarea` 0.7 supports ratatui up to 0.29, and `ratatui-image` 11
needs ratatui 0.30. `ratatui-textarea` is the continuation of the same
widget under the ratatui organisation and supports ratatui 0.30. It uses
the crossterm version ratatui re-exports, so the whole tree has one
crossterm.

## Amendment (2026-10-14, ADR 35)

The text fields render full Markdown through patine (ADR 34, ADR 35),
and `pulldown-cmark` goes.

## Amendment (2026-10-14)

`serde_path_to_error` wraps the typed parse of a session, so a shape
error names the path of its field, as in `questions[1].context: invalid
type: map, expected a sequence`. `serde_json` alone names no field when
it parses from a `Value`, and a session is parsed from one because
`parse_session` checks the version and the context entry types on the
raw JSON first. In a file with several questions, an agent could not
tell which question held the wrong value.

The path ends at a context entry for the fields inside it, as in
`questions[0].context[0]: invalid type: null, expected a string`. An
entry is an internally tagged enum: serde reads it into a buffer first
and parses the variant's fields from that buffer, where
`serde_path_to_error` no longer follows the path.

## Amendment (2026-10-15, spec section 7.7)

`rustix` with its `event` feature, on Unix only, waits for the answer to
the passthrough probe with `poll` on stdin. crossterm already depends on
the same version, so it adds no crate to the build; `poll` gives a
timeout without leaving a thread blocked in a read.

## Amendment (2026-10-16, graphics module)

The passthrough probe is gone. It was built for the blocked reader of
ratatui-image 11.1.0, and with 12.0.0-rc.2 the graphics query itself
tells whether a tmux asqr cannot ask passes sequences on. `rustix`'s
`poll` stays for discarding late answers to the query.

## Amendment (2026-10-16, ratatui-image 12.0.0-rc.2)

`ratatui-image` is pinned to the release candidate `=12.0.0-rc.2`. It
writes the size of a Kitty placement in cells, so asqr does not add it
to the command itself, and reads the terminal's answer to its query
with `poll` on the calling thread, so no reader stays blocked on stdin. Cargo selects a
pre-release only when it is named exactly. The crate's default features
stay off: in this version they include `chafa-dyn`, which needs
pkg-config and a system libchafa.
