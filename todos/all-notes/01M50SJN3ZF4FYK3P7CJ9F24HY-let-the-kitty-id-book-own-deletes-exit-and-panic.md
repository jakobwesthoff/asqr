---
title: "Let the Kitty id book own deletes, exit and panic"
kind: refactor
component: tui
status: blocked
origin: review
---
# Let the Kitty id book own deletes, exit and panic

From the architecture review of image handling on 2026-10-16 (logbook
`99-report-bild-architektur.md`, ledger round 5, findings OA3, OA4, OA5,
OA6, FA2, FA6, FA7). The lead decided to do it after the graphics module
(todo "Gather image handling into a graphics module around an
asqr-owned Graphics value").

## Problem

- On quitting, the ids travel `Encoder::take_all` → `Viewers::take_all_images`
  → `View::take_kitty_images` → `kitty_deletes` → an
  `Rc<RefCell<Vec<u8>>>` shared with `TerminalGuard`; the book keeps a
  second set `held` only for that. Ownership already says which images
  live: every `Image` is owned by the View and its `HeldId` releases on
  drop.
- A panic deletes nothing: the bytes are filled only after `run`
  returns, and ratatui's panic hook (installed by `try_init`) leaves the
  alternate screen before asqr's guard runs, where Ghostty keeps the
  images.
- Dropped ids leave the book inside the draw closure, before ratatui
  flushes; a failed draw or a failed write loses them.
- `Drawn.deleted_images` is a side channel in a struct meant for what a
  draw measured.
- The focus repeat rule is two loose locals in the run loop.
- The tmux flag for deletes is read separately from the one the
  transmissions used.

## Design

- One book per run, `Arc<Mutex<..>>`, created with `Images`: id source,
  `issued`, `dropped`, the focus repeat state, and its tmux flag.
- `KittyDeletes { ids, tmux }` with `ids()` and `to_bytes()`;
  `Host::delete_images(&KittyDeletes) -> io::Result<()>`.
- The run loop takes the dropped ids after `terminal.draw` returned
  (the frame is flushed), writes them, and on a failed write puts them
  back in the book for the exit. `Drawn.deleted_images` goes.
- `View::focus_gained()` (forget, when `resends_on_focus()`) and the
  book's `focus_gained()` / `after_frame()` hold the repeat rule, with
  plain unit tests.
- Exit by drop order: `View` drops first (every `HeldId` releases), then
  a `KittyExit` guard writes everything still in the book while the
  alternate screen is on, then `TerminalGuard`. A comment and a binary
  test pin the order.
- asqr wraps ratatui's panic hook: drain the book (live and released
  ids), pop the title, disable focus/paste, then ratatui's hook. The
  hook uses `try_lock` (a panic while the book is locked must not
  deadlock) and tolerates a poisoned lock; draining is idempotent.
- A failed Kitty encode (`last_encoding_result`, rc.2) releases its id
  without writing a delete.
- ADR 42 updated (exit and panic now covered).

## Relations

- Depends on: [Gather image handling into a graphics module around an asqr-owned Graphics value](01M50SJN3A7R62BKXYG89R29PA-gather-image-handling-into-a-graphics-module-around-an-asqr-owned.md)
