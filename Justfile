# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# The validation pipeline (ADR 11). The pre-commit hook runs it, so a
# commit only lands when every step passes.
check: license-headers fmt clippy test coverage

license-headers:
    scripts/check-license-headers.sh

fmt:
    cargo fmt --check

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-targets

# Coverage is reported, not enforced (ADR 11): review checks that every
# change is covered.
coverage:
    cargo llvm-cov --all-targets --summary-only

# Points git at the versioned hooks in .githooks/. Run once per clone.
setup:
    git config core.hooksPath .githooks
