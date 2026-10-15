---
title: "Move to ratatui-image 12.0.0-rc.2 and drop the Kitty placement workaround"
kind: chore
component: tui
origin: upstream
---
# Move to ratatui-image 12.0.0-rc.2 and drop the Kitty placement workaround

The user asked for this todo while waiting for a `ratatui-image` release since 2026-10-03. On 2026-10-15 the user decided to move to the release candidate `=12.0.0-rc.2` (2026-10-05), which contains the fix (commit `2d7d7f6` is an ancestor of the `v12.0.0-rc.2` tag), and to keep `ratatui-image` rather than draw images with asqr's own code; the evaluation and its adversarial review are in the logbook `~/playground/2026_10_15_asqr-maskottchen/99-eval-ratatui-image.md` and `99-review-impl-ledger.md` (round 3). Do it after the Kitty-delete item (todo 01M50PJX24T6G6V3DNZKM56K0J) has landed. The emblem item, which draws Talqi in text cells and took it off ratatui-image (ADR 43), has landed.

## Why the workaround exists

Images were cropped when the terminal's cell size differed from the one
asqr detected at start: after a font size change, or in tmux with
clients of different cell sizes. `ratatui-image` 11.1.0 creates the
Kitty virtual placement without `c=`/`r=`, so the terminal sizes it from
its own cells instead of the placeholder cells. Confirmed in Ghostty on
2026-10-03, with and without tmux.

Upstream fixed it in PR
<https://github.com/ratatui/ratatui-image/pull/215> (commit `2d7d7f6`,
merged 2026-10-03 15:41 UTC). No release contained it on that day; the
newest was 12.0.0-rc.0, which predates the merge. Our own PR #219 with
the same fix was closed as a duplicate, as #215 had been open since
2026-10-01 and we had not searched for it.

A `[patch.crates-io]` pointing at a fork would fix local builds only, as
`cargo publish` drops patches (checked with `cargo package`). So asqr
0.9.2 carries the workaround `size_kitty_placement` in
`src/tui/render/images.rs`: after the image widget has drawn, it adds
`c=`/`r=` to the placement command in the buffer. It relies on the exact
text `ratatui-image` writes, so `Cargo.toml` pins the crate to
`=11.1.0`. The function's doc comment explains the details.

## What to do

- Pin `ratatui-image = "=12.0.0-rc.2"` (cargo selects a pre-release only
  when named exactly). Change only the version string in `Cargo.toml`:
  rc.2's default features include `chafa-dyn`, which needs pkg-config and
  a system libchafa, so `default-features = false, features =
  ["crossterm", "image-defaults"]` must stay; say so in the commit message.
- Remove `size_kitty_placement`, its constants and its call, following
  its doc comment. `a_kitty_image_is_placed_over_exactly_its_placeholder_cells`
  in `src/tui/render/screen.rs` must stay green without it.
- Adapt the Kitty constructor the delete item added: rc.2's
  `StatefulKitty::new(id, is_tmux, compress, smo)` takes a fourth
  argument, from `Capability::KittySharedMemory`. rc.2 may transmit
  through shared memory (`t=s`) locally, so tests that check exact
  transmissions adapt.
- rc.2 returns a half-block `Picker` instead of an error when the
  terminal answers nothing (picker.rs:164-175), so asqr's log line
  "no graphics protocol detected" (`src/terminal.rs`, around l.96) never
  fires. Log the half-block reason from `protocol_type()` and empty
  `capabilities()` instead, and adapt tests that expect the error. Only a
  terminal that answers nothing at all loses the iTerm2 protocol hinted
  by the environment; a missing cell-size answer already meant half
  blocks in 11.1.0.
- Drain stdin after the query. rc.2 reads the answer with `poll` on the
  calling thread and stops at the first `ESC[0n` or at its deadline
  (2000 ms by default); everything left in fd 0 is read by crossterm's
  event thread, which parses a Kitty reply such as `ESC _Gi=31;OK ESC \`
  as key events, and the digits in it pick options. This happens when a
  reply arrives after the deadline, or when several tmux clients show the
  pane and each terminal answers. Right after `Picker::from_query_stdio`
  returns and before the event thread starts, poll stdin until about
  150 ms pass without a byte (capped at about 1 s) and discard what
  arrives; do it when the query timed out or tmux reports more than one
  attached client, so a lone answering terminal pays nothing. Handle
  EINTR as `passthrough_answers` does. Tests: a pipe test with a second
  reply burst after the first `ESC[0n`, one with a reply after the
  deadline, and a PTY test where a late reply does not pick an option.
  Spec 7.7 states the remaining risk (a reply later than the timeout plus
  the cap).
- Under tmux 3.4 or later, when the detected protocol is Kitty, set
  `allow-passthrough all` for asqr's pane instead of `on` (user decision,
  2026-10-15), and restore the previous value on exit as
  `PassthroughRestore` does; Kitty transmissions and deletes then also
  reach the terminal from a hidden pane. Sixel and iTerm2 stay at `on`:
  tmux writes passthrough unpositioned, and from a hidden pane they would
  paint over the visible window. Do not touch `input-buffer-size` (a
  server option). The drain above is a precondition, since with `all`
  more clients answer the query. ADR 24 and spec 7.7 change accordingly.
- Reword spec 7.7 and the doc of `may_query_protocol`: with rc.2 the
  gate and the probe are justified by the full timeout and by late
  replies read as keys, no longer by a reader that keeps taking keys.
- Notes on rc.2 behaviour (review FA7, checked against the v12.0.0-rc.2
  sources): `size_kitty_placement` must go, not just may: left in, it
  would write `c=`/`r=` twice. On a query timeout rc.2 skips its termios
  restore (picker.rs:558-560); harmless, since crossterm's raw mode is
  the state it would restore and `ratatui::restore` handles it at exit.
  Do not opt into Kitty shared memory (`smo` stays false): it is probed
  only when asked for, works only locally, and per-image shm objects are
  left for the terminal to unlink.
- Consider a longer query timeout through
  `Picker::from_query_stdio_with_options` (`QueryStdioOptions::timeout_ms`,
  e.g. 5000 ms): the deadline is only reached when no answer comes at
  all, and a longer one shrinks the window for late answers that the
  drain cannot catch; the two belong together. Under tmux with `on`, ask
  `#{window_active}` and `#{pane_active}` with the existing
  `display-message` query and skip the query in a hidden pane (it would
  be dropped and cost the full timeout).
- Give context images a fresh Kitty id when their pane size changes:
  ratatui-image re-encodes on resize under the same id (rc.2
  kitty.rs:160-166), which Ghostty blanks during the transfer. Before
  drawing, if `ResizeEncodeRender::needs_resize` says a resize is due,
  build a new protocol with a fresh id through the delete item's
  constructor and queue the old id through its drop path (about 15 lines;
  the decoded source image is kept next to the protocol).
- Scale context images with a real filter: `images.rs` passes
  `Resize::Fit(None)` / `Resize::Scale(None)`, which means Nearest; pass
  e.g. `Some(FilterType::Triangle)` and check the snapshots.
- Read `StatefulProtocol::last_encoding_result` right after the frame is
  drawn and log an encode error (today a failed encode leaves placeholder
  cells or an empty area without a trace); a failed Kitty encode releases
  its id without writing a delete for an image that was never sent.
- PTY acceptance tests (`tests/tui_pty.rs`, extend `answer_the_query`):
  answering as Kitty with a cell size, the output carries `a=T,U=1` with
  `c=`/`r=`; with no answer, a key typed after the deadline reaches asqr
  (the blocked-reader bug of todo 01M4Y6MS, gone in rc.2).
- Manual check across terminals before release, logging the detected
  protocol per row (`src/terminal.rs` "graphics protocol detected"):
  Ghostty, kitty, WezTerm (and WezTerm over ssh), iTerm2, foot or
  `xterm -ti vt340`, Konsole, VS Code, Windows Terminal, NO_COLOR with
  Kitty, each with and without tmux, and tmux with two clients attached.
  asqr.log on the author's machine shows only half-block runs inside tmux
  on 2026-10-15, so the Kitty path needs this deliberately.
- Correct spec 7.7 where it names WezTerm among the Kitty terminals:
  ratatui-image gives WezTerm the iTerm2 protocol.
- Check 01M4Y6MS afterwards: rc.2 reads the answer with `poll` on the
  calling thread; what may remain is the `tmux set -p allow-passthrough
  on` that `PassthroughRestore` undoes.
- The fork `jakobwesthoff/ratatui-image` and its branches
  `kitty-placement-size` and `kitty-placement-size-v11` are no longer
  used and can be deleted (the user's call, outside this repository).

## Later, not part of this todo

- Propose upstream an option to skip `tmux set -p allow-passthrough on`
  (no AI attribution in commits: the maintainer rejects it).
- If upstream refuses that option, 12.0.0 stalls past a date the user
  sets, or a protocol bug is not fixed upstream, owning the image code
  (the evaluation's option (c)) is the documented fallback.
