---
title: "Open text and diff items with the system opener"
kind: feature
component: tui
status: blocked
origin: discussion
tags: [ux]
---
# Open text and diff items with the system opener

Decided with the user on 2026-10-14 (asqr session
`01M4XFKQ2V7NK2NWKAP9QEEZB2`) while designing the context pane. In its
first phase, `o` opens only images, in the system viewer. Afterwards
`o` opens text and diff items too, the same way: through the system's
opener (`open` on macOS, `xdg-open` on Linux), so the person's
configured app for the file type opens it.

## Proposal

- An item with a `path` opens that file.
- Inline text and diffs are first written to a temporary file with a
  fitting extension: the language's (`.rs`, `.md`) for text, `.diff`
  for a diff, rendered as a unified diff from the old and new text.
- asqr stays on screen; nothing hands the terminal over, unlike a pager
  or editor would.

## Open questions

- Where the temporary files live and when they are removed (the queue's
  cache directory, removed with the session or on quit).

## Relations

- Depends on: [Show text entries in the context pane with syntax highlighting](01M4XHQSD1EM17QD87CFB3VWMG-show-text-entries-in-the-context-pane-with-syntax-highlighting.md)
- Depends on: [Show diff entries in the context pane as a unified diff](01M4XHQSDRCG7M60ZQ5M2D530J-show-diff-entries-in-the-context-pane-as-a-unified-diff.md)
