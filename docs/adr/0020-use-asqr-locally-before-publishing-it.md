# 20. Use asqr locally before publishing it

Date: 2026-09-27

## Status

Superseded by [26. Release through a tag-triggered workflow](0026-release-through-a-tag-triggered-workflow.md)

## Context

crates.io is the intended release channel. Prebuilt binaries and a
Homebrew tap were also considered.

## Decision

asqr is used locally first, installed from the repo. The user decides
when to make the repository public and publish on crates.io. Nobody
publishes before that.

## Consequences

Until then the crate name `asqr` on crates.io is not reserved. The
check on 2026-09-27 found it free.
