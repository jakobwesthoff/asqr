# 17. Require Rust 1.97

Date: 2026-09-27

## Status

Accepted

## Context

The Rust 2024 edition works from 1.85. That floor is old, and the user
prefers a newer one.

## Decision

`Cargo.toml` declares `rust-version = "1.97"`, the stable release of
2026-07-07 and the current stable when development started. CI tests on
stable. The floor is raised whenever the code needs something newer.

Rejected: 1.85, 1.90, and a separate CI job on the minimum version.

## Consequences

Users need a recent toolchain, which is normal for a tool installed with
`cargo install`. Accidental use of features newer than 1.97 is not caught
by CI until the floor is raised.
