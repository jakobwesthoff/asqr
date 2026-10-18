# 20. Use asqr locally before publishing it

Date: 2026-09-27

## Status

Superseded by [26. Release through a tag-triggered workflow](0026-release-through-a-tag-triggered-workflow.md)

## Context

crates.io is the intended release channel. Prebuilt binaries and a
Homebrew tap were also considered.

## Decision

asqr is used locally first, installed from the repo. Making the
repository public and publishing on crates.io is a later step. Nobody
publishes before that.

## Consequences

Until then the crate name `asqr` on crates.io is not reserved. The
check on 2026-09-27 found it free.
