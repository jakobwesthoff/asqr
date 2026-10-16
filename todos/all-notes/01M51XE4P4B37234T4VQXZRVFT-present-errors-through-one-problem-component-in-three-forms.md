---
title: "Present errors through one problem component in three forms"
kind: improvement
component: tui
impact: high
origin: request
tags: [ux, field-test]
---
# Present errors through one problem component in three forms

Jakob, 2026-10-16, after testing missing, unreadable and unsupported
context files: errors should show in red with a mark, and asqr should
present failures in one consistent way through a component of its own.
Preparation (inventory, renders, prototype) in
`~/playground/2026_10_15_asqr-maskottchen/vorbereitung/errors/`
(`inventory.md`, `look-*.png`, `invalid-*.png`, `prototype.diff`, switch
`ASQR_PROTO_ERR=card`).

## Today

- A context entry that cannot be shown draws a placeholder through the
  shared `Placeholder` (`Unreadable::placeholder` in
  `src/tui/render/source_file.rs`; `ImageViewer::new` in `viewer.rs` uses
  the same): a dim heading such as `code not shown`, the absolute path
  wrapped at the pane's edge, also inside a word, and a dim reason.
- An invalid session shows only as a status line notice; the list of
  validation errors is joined with "; " (`src/tui/inbox.rs:163-167`)
  and cut at the edge, so the full reason shows nowhere in asqr.
- The diff placeholder does not say which side failed (`diff.rs:75`).
- `✗` already marks "rejected" in the status line (ADR 38), a neutral
  notice.

## Decisions (Jakob, 2026-10-16, asqr session 01M51Z480PPQV9E71ATGD0MM1R)

- One component, `Problem`, in three forms: a card for context entries,
  a line for the status line, a dialog.
- A context entry that cannot be shown draws a card with a red rounded
  frame in the pane.
- Talqi never appears with errors.
- Reasons keep their original wording (no plain-language rewrite).
- Paths: file name and folder on lines of their own, the home folder as
  `~`, wrapped after a `/`, never shortened.
- An invalid session gets a dialog that lists its errors, framed in
  yellow (the warning role): asqr answered it with an error result.
- Marks in the status line: `✗` for errors, `!` for warnings; "✗
  rejected" stays as it is.
- A broken entry is marked in the pane's header only. Jakob asked why
  the pane would not be enough; the one case it is not is the one-line
  hint of a small terminal (ADR 30), where no viewer is built. Left out
  unless he asks for it.

## Design (decided by the lead)

- Inside the card: a blank line between the reason and the file block,
  none between file name and folder.
- `Unreadable` carries the kind of failure (not found, permission
  denied, directory, not a regular file, relative path, binary, image
  format unsupported, image damaged, other) instead of only a string,
  mapped from `io::ErrorKind` and the `ImageError` variants, so the
  header mark and the dialog do not match message text.
- The diff card names the side that failed.
- The original message is also logged once with `tracing::warn` where
  the viewer builds the card.
- The pane header learns of the failure from the viewer
  (`Viewer::failed()` defaulting to `false`, or an accessor on
  `Content`).
- A card taller than the pane scrolls; it is at most 60 columns wide.
- The validation errors stay a `Vec` from the inbox to the dialog. The
  dialog closes on the next key, and that key does nothing else,
  matching notices.
