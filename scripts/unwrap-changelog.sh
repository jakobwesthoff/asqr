#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Prints the changelog with each paragraph and list item on one line.
# CHANGELOG.md is wrapped for reading in a terminal, but GitHub shows
# every line break in release notes as a break, so the release workflow
# unwraps it before taking a version's section from it.
#
# A line continues the one before it unless it is blank, a heading, a
# list item or inside a fenced code block, which is printed as it is.
set -euo pipefail

awk '
  function flush() {
    if (pending != "") print pending
    pending = ""
  }
  /^```/ { flush(); fenced = !fenced; print; next }
  fenced { print; next }
  /^[[:space:]]*$/ || /^#/ { flush(); print; next }
  /^[[:space:]]*([-*+]|[0-9]+\.)[[:space:]]/ { flush(); pending = $0; next }
  {
    if (pending == "") {
      pending = $0
    } else {
      line = $0
      sub(/^[[:space:]]+/, "", line)
      pending = pending " " line
    }
  }
  END { flush() }
' "${1:?usage: unwrap-changelog.sh CHANGELOG.md}"
