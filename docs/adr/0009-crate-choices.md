# 9. Crate choices

Date: 2026-10-16

## Status

Accepted

Amended by [35. Render the text fields as full Markdown through patine](0035-render-the-text-fields-as-full-markdown-through-patine.md)

Amended by [56. Show text with ANSI colours as a context entry](0056-show-text-with-ansi-colours-as-a-context-entry.md)

## Context

The tool uses clap, anyhow and thiserror, and other crates where they
are needed.

## Decision

| Need | Crate |
|---|---|
| Command line | `clap` with derive |
| Typed errors in the format and queue code | `thiserror` (validation errors name the field) |
| Errors at the binary's edges | `anyhow` with `.context()` |
| TUI | `ratatui`, `crossterm` through ratatui's re-export, `ratatui-textarea`, `ratatui-image` |
| Images | `image` for decoding |
| Markdown and highlighting | `patine` (ADRs 34 and 35) |
| Diffs | `similar` (ADR 33) |
| Watching | `notify` with `notify-debouncer-full` |
| Paths | `directories` |
| Format | `serde`, `serde_json`, `serde_path_to_error`, `schemars` |
| Atomic writes | `tempfile` |
| Timestamps | `jiff` |
| Logging | `tracing` and `tracing-subscriber`, into a file in the platform cache directory |
| System calls, Unix only | `rustix` with its `event` and `fs` features |
| Session ids | `ulid` |
| Tests | `insta`, `assert_cmd` |

Crates are added with `cargo add` when the first test needs them.

- **`ratatui-textarea`** edits the text fields. It is the continuation of
  `tui-textarea` under the ratatui organisation and supports ratatui
  0.30, which `ratatui-image` needs; `tui-textarea` 0.7 supports ratatui
  up to 0.29. It uses the crossterm version ratatui re-exports, so the
  whole tree has one crossterm.
- **`ratatui-image`** is pinned to the release candidate `=12.0.0-rc.3`,
  as Cargo selects a pre-release only when it is named exactly. It
  writes the size of a Kitty placement in cells, so asqr does not add it
  to the command itself, and reads the terminal's answer to its query
  with `poll` on the calling thread, so no reader stays blocked on stdin.
  Its default features stay off: in this version they include
  `chafa-dyn`, which needs pkg-config and a system libchafa.
- **`rustix`** discards late answers to the graphics query with `poll`
  on stdin, and opens files without waiting on a FIFO (ADR 46).
  crossterm already depends on the same version, so it adds no crate to
  the build; `poll` gives a timeout without leaving a thread blocked in
  a read.
- **`serde_path_to_error`** wraps the typed parse of a session, so a
  shape error names the path of its field, as in `questions[1].context:
  invalid type: map, expected a sequence`. `serde_json` alone names no
  field when it parses from a `Value`, and a session is parsed from one
  because `parse_session` checks the version and the context entry types
  on the raw JSON first. In a file with several questions, an agent
  could not tell which question held the wrong value otherwise. The path
  ends at a context entry for the fields inside it, as in
  `questions[0].context[0]: invalid type: null, expected a string`. An
  entry is an internally tagged enum: serde reads it into a buffer first
  and parses the variant's fields from that buffer, and
  `serde_path_to_error` does not follow the path into it.

## Consequences

The tracing output goes to a file, because the TUI owns the terminal.
