---
title: "Design how asqr presents errors and unreadable context"
kind: investigation
component: tui
status: needs-discussion
origin: request
tags: [ux, field-test]
---
# Design how asqr presents errors and unreadable context

Jakob, 2026-10-16, after testing missing, unreadable and unsupported
context files: such errors should show in red, perhaps with an icon, and
the whole formatting of these messages could use a redesign; he wants to
talk about how asqr shows error dialogs in general and build a ratatui
component for them.

## Today

A context entry that cannot be shown draws a dim placeholder: a heading
such as `code not shown`, the absolute path wrapped at the pane's edge
in the middle of a word, and the operating system's message, such as
`No such file or directory (os error 2)` (`Unreadable::placeholder` in
`src/tui/render/source_file.rs`; the image placeholder in
`src/tui/render/`). The design guide has an Error role (red, "a
failure") and requires that meaning never depends on colour alone.

## Ideas raised so far (2026-10-16)

- A red `✗` and heading, path dim; or a card with a red rounded frame,
  centred in the pane, reason in bold, file name, folder dim; or the
  card with Talqi holding a magnifier as a text-cell emblem above it
  when the pane has room (needs a second emblem pose).
- Reasons in plain words (`file not found`, `no permission to read`,
  `a directory, not a file`, `not an image asqr can show`), the
  original message in the log.
- Paths wrapped at `/`, the home directory as `~`, `…` in the middle
  when too long.
- A red `✗` in the pane's header.

## To decide with Jakob

Which form, whether it is one component for every error asqr shows
(placeholders, status line failures, error dialogs), and whether Talqi
appears.
