# 83. Set which events reach the log through ASQR_LOG

Date: 2026-10-30

## Status

Accepted

Amends [9. Crate choices](0009-crate-choices.md):
`tracing-subscriber` with its `env-filter` feature, which brings in
`matchers`

Amends [15. Configure through flags and environment and log into the cache directory](0015-configure-through-flags-and-environment-and-log-into-the-cache-directory.md):
`ASQR_LOG` sets which events reach the log

## Context

- asqr appends to `asqr.log` and never rotates it.
- The log takes asqr's `tracing` events and the records of the
  libraries that log through the `log` crate, such as notify and mio.
  Without a filter it takes those at `info` and above, and none at
  `debug` reach it.
- Each frame that sends a Kitty image logs its bytes and times, a
  resize that encodes the image again included.

## Decision

- `ASQR_LOG` sets which events reach the log, in the directive syntax
  of `tracing-subscriber`'s `EnvFilter`: a level, or comma-separated
  `target=level` directives such as `info,asqr=debug`. Spaces around a
  directive do not count. asqr does not read `RUST_LOG`.
- A level alone applies to every target, the libraries included;
  `info,asqr=debug` adds asqr's own `debug` events only.
- Unset, empty or not UTF-8, `ASQR_LOG` lets `info` and above through.
- A value that does not parse stops no command and prints nothing, as
  the TUI owns the terminal. The log then takes `info` and above, and a
  warning in it names `ASQR_LOG`, the value and the parse error.
- The line of a frame that sends a Kitty image is logged at `debug`: it
  is a measurement from every frame that sends an image, not an event
  of everyday use, and the log is never rotated. The sweep of the files
  `o` opened (ADR 78) logs at `debug` too; every other line keeps its
  level.
- The filter is the `env-filter` feature of `tracing-subscriber`, which
  brings in the crate `matchers`.

## Consequences

- A value with any directive replaces the `info` default:
  `ASQR_LOG=asqr=debug` lets nothing but asqr's events through.
- A bare word that is no level names a target, so a misspelled level
  such as `ASQR_LOG=degub` leaves nothing of asqr in the log, and no
  warning either.
