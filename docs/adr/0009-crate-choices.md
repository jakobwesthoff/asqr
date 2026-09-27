# 9. Crate choices

Date: 2026-09-27

## Status

Accepted

## Context

The user asked for clap, anyhow and/or thiserror, and other
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

## Amendment (2026-09-27, phase 4)

`ratatui-textarea` replaces `tui-textarea` for the text fields.
`tui-textarea` 0.7 supports ratatui up to 0.29, and `ratatui-image` 11
needs ratatui 0.30. `ratatui-textarea` is the continuation of the same
widget under the ratatui organisation and supports ratatui 0.30. It uses
the crossterm version ratatui re-exports, so the whole tree has one
crossterm.
