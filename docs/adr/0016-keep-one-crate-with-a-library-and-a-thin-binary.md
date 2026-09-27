# 16. Keep one crate with a library and a thin binary

Date: 2026-09-27

## Status

Accepted

## Context

The logic has to be testable without a terminal, and the planned MCP
server will reuse the format and queue code.

## Decision

asqr is one crate. `src/lib.rs` holds the logic and `src/main.rs` only
parses arguments and starts the terminal.

Rejected: a workspace with `asqr-core` and `asqr` from the start.

## Consequences

When a second consumer needs the library, it can be split into a
workspace without changing the code inside it.
