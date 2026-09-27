#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Names every tool `just check` and `just coverage` need that this machine
# lacks, with the command that installs it, so a fresh clone does not fail
# its first commit with an error from inside the hook. It installs nothing
# itself.
set -euo pipefail

missing=0

need() {
  local what=$1 install=$2
  echo "missing: $what (install: $install)" >&2
  missing=1
}

command -v cargo >/dev/null || need cargo "https://rustup.rs"
command -v just >/dev/null || need just "cargo install just"
cargo llvm-cov --version >/dev/null 2>&1 || need cargo-llvm-cov "cargo install cargo-llvm-cov"

# cargo-llvm-cov needs the LLVM tools of the active toolchain; without them
# it offers to install them interactively in the middle of a run.
if command -v rustup >/dev/null; then
  rustup component list --installed | grep -q '^llvm-tools' ||
    need llvm-tools-preview "rustup component add llvm-tools-preview"
fi

if [ "$missing" -ne 0 ]; then
  exit 1
fi
echo "all tools present"
