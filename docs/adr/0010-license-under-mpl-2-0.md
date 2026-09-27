# 10. License under MPL-2.0

Date: 2026-09-27

## Status

Accepted

## Context

The tool is meant for many people and projects.

## Decision

asqr is licensed under the Mozilla Public License 2.0 (user's choice).
The Rust default, MIT or Apache-2.0, was offered and not chosen.

## Consequences

Changes to asqr's own files stay open when it is distributed. Programs
that only call asqr or use its file format are not affected.

Every source file carries the notice from Exhibit A of the license in
its opening comment:

    This Source Code Form is subject to the terms of the Mozilla Public
    License, v. 2.0. If a copy of the MPL was not distributed with this
    file, You can obtain one at https://mozilla.org/MPL/2.0/.

`scripts/check-license-headers.sh` enforces this for Rust files, shell
scripts and git hooks as part of `just check`. The full license text is
in `LICENSE`, and `Cargo.toml` declares `license = "MPL-2.0"`.
