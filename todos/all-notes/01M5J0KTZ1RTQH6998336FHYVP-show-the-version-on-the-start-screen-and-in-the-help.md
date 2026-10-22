---
title: "Show the version on the start screen and in the help"
kind: improvement
component: tui
origin: request
tags: [ux]
---
# Show the version on the start screen and in the help

Raised on 2026-10-22 after releasing asqr 0.13.0: nothing in the
terminal interface says which asqr runs, so after an update the person
cannot tell whether the asqr in their terminal is the new one. The
version is to show on the screen asqr shows after it starts and in the
help that `?` opens.

## Where it is today

- After it starts, with an empty queue, asqr shows Talqi and the
  sentence "Nothing to answer. New sessions show up here as soon as they
  arrive." (`SENTENCE` in `src/tui/render/empty.rs`).
- The help is an overlay with a title in its frame (`draw_help` in
  `src/tui/render/keys.rs`, `overlay_block` in
  `src/tui/render/overlay.rs`).
- The version is `env!("CARGO_PKG_VERSION")`, as the error for an
  unknown context type already uses.

## Proposal

The version, such as `asqr 0.13.1`, on the start screen and in the
help's frame. Where exactly on each, and in which style, is to be shown
to the user as rendered captures before it is built.

## Decisions

- 2026-10-22, user: the version shows on the screen after the start and
  in the `?` help.
- 2026-10-22, user: it goes into the same patch release, 0.13.1, as the
  ctrl-u and ctrl-d hints.
