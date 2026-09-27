#!/usr/bin/env bash
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.

# Fails when a source file lacks the MPL 2.0 notice (Exhibit A of the
# license), which the license asks to carry in every Source Code Form file.
# Only files tracked or staged in git count, so build output and scratch
# files never trip the check.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

notice='This Source Code Form is subject to the terms of the Mozilla Public'

missing=()
while IFS= read -r -d '' file; do
  # The notice belongs in the file's opening comment, so only the head is
  # searched; a quote of the sentence further down does not count.
  if ! head -n 5 "$file" | grep -qF "$notice"; then
    missing+=("$file")
  fi
done < <(git ls-files -z --cached --others --exclude-standard -- '*.rs' '*.sh' '.githooks/*')

if [ ${#missing[@]} -gt 0 ]; then
  echo "Missing the MPL 2.0 notice in the first 5 lines:" >&2
  printf '  %s\n' "${missing[@]}" >&2
  exit 1
fi
