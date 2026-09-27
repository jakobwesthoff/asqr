# 15. Configure through flags and environment and log into the cache directory

Date: 2026-09-27

## Status

Accepted

## Context

Version 1 has few settings: the queue, notifications and the bell. The
TUI owns the terminal, so logs cannot go to stdout or stderr.

## Decision

- Settings come from command-line flags and environment variables only,
  for example `--no-bell`, `--no-notify`, `--queue` and `ASQR_QUEUE`.
  There is no config file in version 1.
- The log goes to a file in the platform cache directory (`directories`),
  and `asqr paths` shows where.

Rejected: a TOML config file from the start. For the log: the queue root,
and logging only when asked with `--log`.

## Consequences

A config file can come later without changing the flags. Logs are
disposable, as a cache directory implies.
