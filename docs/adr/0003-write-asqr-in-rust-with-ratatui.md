# 3. Write asqr in Rust with ratatui

Date: 2026-09-27

## Status

Accepted

## Context

The candidates were Rust and TypeScript on Bun. The TUI has to show
images inline, edit text over several lines, and start fast as a
long-running tool in its own terminal.

## Decision

asqr is written in Rust (edition 2024) with `ratatui` and `crossterm`.

- `ratatui-image` shows images through the Kitty graphics protocol, the
  iTerm2 protocol or Sixel, and picks the one the terminal supports.
- `tui-textarea` edits text over several lines.
- The result is one small binary that is installed with `cargo install`.

Rejected: Bun with Ink or OpenTUI. Ink has no image support and only
simple text input, and OpenTUI is young. A compiled Bun binary carries
the runtime and weighs several tens of MB.

## Consequences

Trying out UI changes takes longer than it would in TypeScript. The
crates are recorded in ADR 9.
