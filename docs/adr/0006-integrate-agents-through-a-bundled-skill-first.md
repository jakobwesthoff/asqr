# 6. Integrate agents through a bundled skill first

Date: 2026-09-27

## Status

Accepted

## Context

AI coding agents are the main askers. They could use asqr through a skill
that explains the files and the CLI, or through an MCP server.

## Decision

Version 1 ships a skill at `skills/asqr/SKILL.md`. The skill explains:

- the file format
- `asqr paths`
- `asqr ask --wait` running in the background
- patterns for longer sessions: batches, follow-up rounds and keeping a
  record

The skill is compiled into the binary. `asqr skill` prints it and
`asqr skill --install <dir>` writes it, so the installed skill always
matches the installed asqr.

An MCP server (`asqr mcp`, stdio) comes later as a thin layer on the same
queue code.

## Consequences

Any agent that can write files and run commands can use asqr without
configuration. Before the MCP server exists, agents get no typed tools.
MCP calls have time limits, so the server will need a submit call and a
wait call.
