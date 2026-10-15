---
title: "Gather image handling into a graphics module around an asqr-owned Graphics value"
kind: refactor
component: tui
status: blocked
origin: review
---
# Gather image handling into a graphics module around an asqr-owned Graphics value

From the architecture review of image handling on 2026-10-16 (logbook
`~/playground/2026_10_15_asqr-maskottchen/99-report-bild-architektur.md`,
ledger round 5, findings OA1, OA2, OA7, OA8, OA10, OA11, OA12, FA1, FA5).
The lead decided to do it, after the move to ratatui-image 12.0.0-rc.2
(todo 01M4169ZBJ9P5QFR20761YE0TH), which touches the same lines.

## Problem

- `ratatui_image::Picker` is passed by value through `View::new`,
  `TabState::new`, `Viewers::new` and `Encoder::new`, and
  60 test sites build `Picker::halfblocks()`, among them modules with no
  business with images (app.rs, inbox.rs, run.rs). Every picker built
  inside tmux runs `tmux set -p allow-passthrough on` on the pane; the
  `just test` recipe works around that with `TERM=xterm-256color
  TERM_PROGRAM=` (commit fa9c8c7).
- `src/tui/render/images.rs` holds four concerns: image load and draw,
  the Kitty id book, the delete command plus the c=/r= workaround, and
  terminal detection (`TerminalNames`, `may_query_protocol`,
  `passthrough_answers`), which is re-exported for the binary. The other
  half of detection (`PassthroughRestore`, `tmux_state`, `run_tmux`,
  `probe_passthrough`) lives in `src/terminal.rs`, against ADR 16 and
  that file's own header. The tmux-like `TERM` test exists twice
  (images.rs, terminal.rs); passthrough wrapping three times (alert.rs,
  images.rs via ratatui-image's parser, a pre-wrapped constant);
  `TerminalNames` and `AlertTerminal` read the same variables with
  different empty-`TMUX` rules.
- `Image` can be made three ways, one of which (`Image::from_picture`)
  bypasses the Kitty id book; `Image::draw` has no access to the book or
  the source picture, so the rc.2 fresh-id-on-resize has no home.
- Each `Encoder::new` creates its own id book although the book is meant
  to span the run.
- `must_clear` (screen.rs) clears the terminal for every protocol
  although ADR 31 gives Sixel and iTerm2 as the reason; the focus-gain
  forget (viewer.rs, run.rs) runs for every protocol and terminal
  although only Kitty under tmux with `allow-passthrough on` loses
  transfers.
- Kitty test scaffolding (a Kitty picker, U+10EEEE, finding the
  transmitting cell, reading ids back) is copied across six files.

## Target

A library module `src/graphics/` next to `tui`:
- `mod.rs`: `Graphics` (Copy; the detected facts: protocol, cell size,
  tmux passthrough as `Passthrough::{None, WhileVisible, Always}`, Kitty
  compression) with `from_picker(&Picker)`, `HALF_BLOCKS`,
  `cells_are_skipped()` (Sixel, iTerm2) and `resends_on_focus()` (Kitty
  and `WhileVisible`); `Images` (Clone; `Graphics` plus the one id book)
  with `encode`, `load`. Created once in the binary and passed to
  `View::new`; tests use `Images::half_blocks()`, `Images::kitty()`.
- `env.rs`: `TerminalEnv` with named `in_tmux_pane()` (TMUX non-empty)
  and `tmux_like_term()` (what ratatui-image tests), replacing
  `TerminalNames` and `AlertTerminal`'s own reading; one
  `passthrough(&[u8]) -> Vec<u8>` used by alerts, the probe and deletes.
- `detect.rs`: the query decision, `PassthroughRestore` with its tests,
  the answer reader; the binary keeps only the real I/O closures.
- `image.rs`: `Image` owns picture source (for Kitty), protocol and held
  Kitty id; built only through `Images`; `draw` holds the fresh id on
  resize; one function builds all four `StatefulProtocolType`s.
- `kitty.rs`: the id book, `HeldId`, the delete bytes.
- `testing.rs` (`#[cfg(test)]`): `kitty()`, `PLACEHOLDER`,
  `transmission`, `placeholder_cells`, `kitty_ids_on_screen`.
- `must_clear` keyed on `cells_are_skipped()`, the focus forget on
  `resends_on_focus()`; the focus test moves to a Kitty `Images`.
- Once tests no longer build a `Picker`, the `just test` environment
  workaround can go.

## Notes

- No behaviour change except the two policy keys above, which are
  deliberate (fewer clears and re-reads under half blocks and outside
  tmux `on`).
- The id book's own redesign (deletes after the flush, exit by drop
  order, panic hook, focus repeat) is the follow-up todo "Let the Kitty
  id book own deletes, exit and panic".

## Relations

- Depends on: [Move to ratatui-image 12.0.0-rc.2 and drop the Kitty placement workaround](01M4169ZBJ9P5QFR20761YE0TH-move-to-ratatui-image-1200-rc2-and-drop-the-kitty-placement-workaround.md)
