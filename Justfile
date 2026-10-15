# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# The validation pipeline (ADR 11, amended by ADR 21). The pre-commit hook
# runs it, so a commit only lands when every step passes. Coverage is kept
# out of it: the instrumented build compiles every dependency a second
# time, which would slow down each commit after a new crate arrives.
check: license-headers fmt clippy test

license-headers:
    scripts/check-license-headers.sh

fmt:
    cargo fmt --check

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

# `--all-targets` leaves out doctests, so they run as a second step.
test:
    cargo test --all-targets
    cargo test --doc

# Coverage is reported, not enforced (ADR 21): review checks that every
# change is covered. CI runs it on every push.
coverage:
    cargo llvm-cov --all-targets --summary-only

# Run once per clone: points git at the versioned hooks in .githooks/ and
# names the tools the pipeline needs but cannot find.
setup:
    git config core.hooksPath .githooks
    scripts/check-tools.sh
