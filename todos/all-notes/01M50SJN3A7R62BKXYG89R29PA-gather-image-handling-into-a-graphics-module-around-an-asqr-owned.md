---
title: "Gather image handling into a graphics module around an asqr-owned Graphics value"
kind: refactor
component: tui
origin: review
---
# Gather image handling into a graphics module around an asqr-owned Graphics value

From the architecture review of image handling on 2026-10-16 (logbook
`~/playground/2026_10_15_asqr-maskottchen/99-report-bild-architektur.md`,
ledger round 5, findings OA1, OA2, OA7, OA8, OA10, OA11, OA12, FA1, FA5).
The lead decided to do it after the move to ratatui-image 12.0.0-rc.2,
which touched the same lines and has landed.

## Problem

- `ratatui_image::Picker` is passed by value through `View::new`,
  `TabState::new`, `Viewers::new` and `Encoder::new`, and
  60 test sites build `Picker::halfblocks()`, among them modules with no
  business with images (app.rs, inbox.rs, run.rs). Every picker built
  inside tmux runs `tmux set -p allow-passthrough on` on the pane; the
  `just test` recipe works around that with `TERM=xterm-256color
  TERM_PROGRAM=` (commit fa9c8c7).
- `src/tui/render/images.rs` holds four concerns: image load and draw,
  the Kitty id book, the delete command, and terminal detection
  (`TerminalNames`, `TmuxState`, `may_query_protocol`,
  `passthrough_answers`, `answers_may_follow_the_query`,
  `discard_late_answers`, `passthrough_all_for_images`), which is
  re-exported for the binary. The other
  half of detection (`PassthroughRestore`, `tmux_state`, `run_tmux`,
  `probe_passthrough`) lives in `src/terminal.rs`, against ADR 16 and
  that file's own header. The tmux-like `TERM` test exists twice
  (images.rs, terminal.rs); passthrough wrapping three times (alert.rs,
  images.rs via ratatui-image's parser, a pre-wrapped constant);
  `TerminalNames` and `AlertTerminal` read the same variables with
  different empty-`TMUX` rules.
- `Image` can be made three ways, one of which (`Image::from_picture`)
  bypasses the Kitty id book. For the fresh Kitty id on resize, a Kitty
  `Image` keeps a clone of its `Encoder` and a second copy of the
  decoded picture in `KittyImage`.
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

## Corrections from the cleanup review after rc.2 (2026-10-16)

Review round 6 on main `f853114` (logbook ledger, OA13 to OA20) changed
the target:
- **Shared environment module.** `TerminalEnv` and `TmuxState` go into
  `src/terminal_env.rs` beside `graphics`, not under it: alerts, the
  emblem's glyph choice (`mascot/glyphs.rs`, which reads about a dozen
  variables through its own closure) and graphics all use them.
  `TerminalEnv` is a snapshot of named variables with `set(name)` (set
  and non-empty), `in_tmux_pane()`, `tmux_like_term()` and a
  `from_pairs` test constructor; `in_tmux_pane` is computed once in
  `terminal.rs` (today four copies of the test exist).
- **`TmuxState`.** The one tmux answer (`TMUX_STATE_FORMAT`, seven
  tab-separated fields) is parsed once into an owned type with named
  accessors; today it travels as a string and is parsed four times, and
  `glyphs.rs` reads the termtype by position through a parser imported
  from `images.rs`. `may_query_protocol` loses its lazy closure; the
  decisions take `Option<&TmuxState>`.
- **Effective passthrough.** `Graphics`' `Passthrough` is filled after the
  start-up setup from the effective value: `Always` when asqr set `all`
  or found it, `WhileVisible` for `on`, `None` outside tmux. One function
  (e.g. `settle_passthrough`) returns it. `passthrough_all_for_images`
  and `answers_may_follow_the_query` take `&Graphics` and
  `Option<&TmuxState>` instead of bools.
- **One poll helper.** `passthrough_answers` and `discard_late_answers`
  share `wait_for_input(input, wait) -> io::Result<Ready>` with
  `Ready::{Input, Timeout, Interrupted}`; each caller keeps its deadline
  policy (the probe keeps its deadline, the discard restarts its quiet
  time after a signal).
- **Encode for the size in `lay_out`, draw a fixed protocol (FA8,
  replaces the re-issue built in 71aedba/f853114).** `Viewers::laid_out`
  calls `Viewer::lay_out` exactly when the body size changes; encode
  there instead of re-deriving the change in `draw`. rc.2's public API
  covers it: `Resize::size_for(&image, font_size, available)` and
  `Resize::resize(..)` (lib.rs:413, 440), `Kitty::new(image, size, id,
  tmux, compress, false)` (protocol/kitty.rs:81) under a fresh id from the
  book (the old `HeldId` drops in the assignment), `Picker::new_protocol`
  for the other protocols, and the stateless `ratatui_image::Image`
  widget to draw. An encode error is handled right there (logged once,
  id forgotten, nothing drawn). `StatefulProtocol` and `StatefulImage`
  leave asqr; the second decoded copy, the `encoded` and `failed` flags,
  the per-frame `needs_resize` and the post-render `last_encoding_result`
  poll go. The test `a_kitty_image_laid_out_at_another_size_is_sent_under_a_new_id`
  moves to `lay_out`. `image.rs` holds this instead of "draw holds the
  fresh id on resize"; the image's state is then just "a protocol or
  nothing", which also covers OA20 (a failed encoding keeps no picture).
- **Drop the passthrough probe (FA10, decided by the lead).** It was
  built for 11.1.0's blocked reader; with rc.2 the query itself is the
  test over ssh. `may_query_protocol` without `TMUX` and with a tmux-like
  `TERM` queries; a 5 s start in the corner "ssh from a tmux pane with
  `on` that is hidden at start" is accepted. `passthrough_answers`, the
  probe constants, `probe_passthrough` (both cfg variants), its patience
  and their tests go; `wait_for_input` then serves the drain only. Spec
  7.7 and ADR 44 say so; todo 01M4Y6MS is adjusted.
- **Drain trigger (FA11).** The late-answer drain runs when the query hit
  its deadline (an `Instant` measured around the picker constructor) or
  tmux reports more than one attached client, instead of "no capability
  found".
- **Test fix (FA12).** `sigterm_sigint_and_sighup_quit_and_give_the_terminal_back`
  starts its stand-in tmux with the old space-separated `"off 1"`; use
  the tab-separated form like its neighbours.
- **Docs and names.** Fix the `images.rs` module doc and the
  `terminal.rs` header; rename `PassthroughRestore` to `PanePassthrough`
  (it snapshots, sets `all`, restores early or on drop).
- **Tests.** The screen-level `a_kitty_image_is_placed_over_exactly_its_placeholder_cells`
  moves to image level as the unit guard on `testing.rs` helpers; the
  PTY test stays.
- Already done by the rc.2 move (do not redo): fresh Kitty id on resize,
  triangle filter, reading `last_encoding_result`.
